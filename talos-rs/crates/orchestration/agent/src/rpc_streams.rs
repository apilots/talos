use super::*;

struct PreparedRun {
    lease: Lease,
    harness: Arc<dyn AgentHarness>,
    auth: NodeAuth,
    session: agent_protocol::AgentSessionContext,
    identity: talos_types::AgentCaller,
    ownership: crate::SessionLease,
    reservation: super::session_gate::Reservation,
}

impl AgentHarnessRpcAdapter {
    pub(super) async fn finish_disconnected(
        &self,
        harness: &Arc<dyn AgentHarness>,
        agent_id: &str,
        identity: &talos_agent_api::AgentTurnRef,
        settlement: impl Future<Output = TalosResult<agent_protocol::AgentRunSettlement>>,
    ) {
        let cleanup = match interrupt_disconnected(harness, agent_id, identity).await {
            Err(error) if error.is_code(ErrorCode::ResourceNotFound) => {
                // Harness sends the receipt before removing its live invocation.
                // A missing invocation alone cannot establish cleanup ownership.
                let confirmed = tokio::select! {
                    biased;
                    receipt = settlement => receipt.is_ok_and(|receipt| confirms_cleanup(&receipt)),
                    () = std::future::ready(()) => false,
                };
                if confirmed {
                    Ok(())
                } else {
                    Err(error)
                }
            }
            outcome => outcome,
        };
        if let Err(error) = cleanup.and_then(|_| self.session_gate.cleaned(identity)) {
            talos_logging::report_error(&error);
            self.cleanup_failure
                .lock()
                .expect("Agent cleanup lock poisoned")
                .get_or_insert(error);
        }
    }

    // Both the library handle and RPC projection use this same admission and
    // settlement owner. Events are forwarded only into a single rendezvous
    // slot so the RPC projection's 32-frame queue remains the backpressure
    // limit, while this owner can still drop the original receiver.
    pub(super) fn guard_run(
        &self,
        lease: Lease,
        harness: Arc<dyn AgentHarness>,
        execution: AgentRunExecution,
        session: agent_protocol::AgentSessionContext,
        caller: talos_types::AgentCaller,
        resources: (
            crate::SessionLease,
            Box<dyn Send>,
            super::session_gate::Reservation,
        ),
    ) -> AgentRunExecution {
        use agent_protocol::{
            AgentAdmission, AgentAdmissionPermit, AgentProgressEventReceiver,
            AgentRunSettlementReceiver,
        };
        use tokio::sync::oneshot;
        let identity = caller.turn_ref();
        let agent_id = caller.agent_id;
        let (events_tx, events_rx) = mpsc::channel(1);
        let (settlement_tx, settlement_rx) = oneshot::channel();
        let (commit_tx, commit_rx) = oneshot::channel();
        let (accepted_tx, accepted_rx) = oneshot::channel();
        let adapter = self.clone();
        let AgentRunExecution {
            admission,
            mut events,
            settlement,
        } = execution;
        tokio::spawn(async move {
            let (ownership, _resource_guard, mut reservation) = resources;
            tokio::select! {
                biased;
                _ = lease.ended() => return,
                _ = events_tx.closed() => return,
                committed = commit_rx => if committed.is_err() { return; },
            }
            let admitted = (|| {
                if lease.revoked.is_cancelled() || lease.expired() {
                    return Err(unauthenticated());
                }
                reservation.consuming(&identity)?;
                ownership.retain_turn(&identity.turn_id);
                adapter
                    .active
                    .lock()
                    .expect("Agent active lock poisoned")
                    .insert(
                        (
                            agent_id.clone(),
                            identity.session_id.clone(),
                            identity.turn_id.clone(),
                        ),
                        lease.owner.clone(),
                    );
                Ok(())
            })();
            if let Err(error) = admitted {
                let _ = accepted_tx.send(Err(error));
                return;
            }
            let consumed = tokio::select! {
                biased;
                _ = lease.ended() => None,
                result = admission.permit.consume() => Some(result),
            };
            match consumed {
                Some(Ok(())) => {
                    let _ = accepted_tx.send(Ok(()));
                }
                Some(Err(error)) => {
                    if error.is_code(ErrorCode::RunCleanupUnconfirmed) {
                        adapter
                            .cleanup_failure
                            .lock()
                            .expect("Agent cleanup lock poisoned")
                            .get_or_insert(error.clone());
                    }
                    if let Ok(Some(state)) =
                        harness.read_session_execution_state(session.clone()).await
                    {
                        if state.active_turn.is_none()
                            && state.history_cursor.completed_turn_id.as_deref()
                                != Some(identity.turn_id.as_str())
                        {
                            drop(reservation);
                            adapter.session_owner.release_turn(&identity);
                            if let Err(cleanup) = adapter.session_gate.reconcile_unadmitted(
                                &session,
                                &identity,
                                state.history_cursor,
                            ) {
                                talos_logging::report_error(&cleanup);
                            }
                        }
                    }
                    let _ = accepted_tx.send(Err(error));
                    adapter
                        .active
                        .lock()
                        .expect("Agent active lock poisoned")
                        .remove(&(agent_id, identity.session_id, identity.turn_id));
                    return;
                }
                None => {
                    drop(events);
                    adapter
                        .finish_disconnected(&harness, &agent_id, &identity, settlement.recv())
                        .await;
                    adapter
                        .active
                        .lock()
                        .expect("Agent active lock poisoned")
                        .remove(&(agent_id, identity.session_id, identity.turn_id));
                    return;
                }
            }
            let mut settlement = Box::pin(settlement.recv());
            let mut events_open = true;
            let mut delivery_open = true;
            loop {
                tokio::select! {
                    biased;
                    _ = lease.ended() => break,
                    event = events.recv(), if events_open => match event {
                        Some(event) => {
                            if delivery_open {
                                tokio::select! {
                                    biased;
                                    _ = lease.ended() => break,
                                    sent = events_tx.send(event) => delivery_open = sent.is_ok(),
                                }
                            }
                        }
                        None => events_open = false,
                    },
                    result = &mut settlement, if !events_open => {
                        let result = match result {
                            Ok(settlement) => {
                                let confirmed = if confirms_cleanup(&settlement) { adapter.session_gate.cleaned(&identity) } else { Ok(()) };
                                let confirmed = match (&settlement, confirmed) {
                                    (agent_protocol::AgentRunSettlement::Finished { .. }, Ok(())) => match harness.read_session_execution_state(session.clone()).await {
                                        Ok(Some(state)) if state.active_turn.is_none() => adapter.session_gate.settled(&session, &identity, state.history_cursor),
                                        Ok(_) => Ok(()),
                                        Err(error) => Err(error),
                                    },
                                    (_, result) => result,
                                };
                                if confirmed.is_ok() && (matches!(settlement, agent_protocol::AgentRunSettlement::Finished { .. }) || adapter.session_owner.hold() == crate::SessionHold::Attached) { adapter.session_owner.release_turn(&identity); }
                                confirmed.map(|()| settlement)
                            }
                            Err(error) => Err(error),
                        };
                        adapter.active.lock().expect("Agent active lock poisoned").remove(&(agent_id, identity.session_id, identity.turn_id));
                        drop(reservation);
                        if let Err(Err(error) | Ok(agent_protocol::AgentRunSettlement::Finished { result:Err(error),.. })) = settlement_tx.send(result) {
                            talos_logging::report_error(&error);
                        }
                        return;
                    }
                }
            }
            drop(events);
            adapter
                .finish_disconnected(&harness, &agent_id, &identity, settlement.as_mut())
                .await;
            adapter
                .active
                .lock()
                .expect("Agent active lock poisoned")
                .remove(&(agent_id, identity.session_id, identity.turn_id));
        });
        AgentRunExecution {
            admission: AgentAdmission {
                events: admission.events,
                permit: AgentAdmissionPermit::new(async move {
                    commit_tx.send(()).map_err(|_| unauthenticated())?;
                    accepted_rx.await.map_err(|_| unauthenticated())?
                }),
            },
            events: AgentProgressEventReceiver::new(events_rx),
            settlement: AgentRunSettlementReceiver::new(settlement_rx),
        }
    }

    pub(super) fn serve_run(
        &self,
        commit: CommitFuture,
        lease: Lease,
        execution: TalosResult<AgentRunExecution>,
    ) -> Response<RpcStream<pb::RunResponse>> {
        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(async move {
            let mut execution = match execution {
                Ok(value) => value,
                Err(error) => {
                    let _ = tx.send(Ok(run_failure(error))).await;
                    return;
                }
            };
            let admission = match harness_pb::encode_admission_events(execution.admission.events) {
                Ok(events) => events,
                Err(error) => {
                    let _ = tx.send(Ok(run_failure(error))).await;
                    return;
                }
            };
            if lease
                .send(
                    &tx,
                    Ok(pb::RunResponse {
                        result: Some(pb::run_response::Result::Admission(Box::new(admission))),
                    }),
                )
                .await
                .is_err()
            {
                return;
            }
            if let Err(error) = commit.await {
                let _ = lease.send(&tx, Ok(run_failure(error))).await;
                return;
            }
            let consumed = tokio::select! {
                biased;
                _ = lease.ended() => return,
                result = execution.admission.permit.consume() => result,
            };
            if let Err(error) = consumed {
                let _ = lease.send(&tx, Ok(run_failure(error))).await;
                return;
            }
            if lease
                .send(
                    &tx,
                    Ok(pb::RunResponse {
                        result: Some(pb::run_response::Result::Accepted(Box::new(pb::Empty {}))),
                    }),
                )
                .await
                .is_err()
            {
                return;
            }
            while let Some(event) = tokio::select! {
                biased;
                _ = lease.ended() => return,
                _ = tx.closed() => return,
                event = execution.events.recv() => event,
            } {
                let frame = harness_pb::encode_event(event).map(|event| pb::RunResponse {
                    result: Some(pb::run_response::Result::Progress(Box::new(event))),
                });
                match frame {
                    Ok(frame) => {
                        if lease.send(&tx, Ok(frame)).await.is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = lease.send(&tx, Ok(run_failure(error))).await;
                        return;
                    }
                }
            }
            let result = execution.settlement.recv().await.and_then(|settlement| {
                if let agent_protocol::AgentRunSettlement::Finished {
                    result: Err(error), ..
                } = &settlement
                {
                    talos_logging::report_error(error);
                }
                harness_pb::encode_settlement(settlement)
            });
            let frame = match result {
                Ok(value) => pb::RunResponse {
                    result: Some(pb::run_response::Result::Settlement(Box::new(value))),
                },
                Err(error) => run_failure(error),
            };
            let _ = lease.send(&tx, Ok(frame)).await;
        });
        Response::new(Box::pin(ReceiverStream::new(rx)))
    }

    async fn prepare_run(
        &self,
        auth: &NodeAuth,
        context: &agent_protocol::AgentContext,
        admission: super::session_gate::RunAdmission,
        coordinator: TalosResult<&str>,
    ) -> TalosResult<PreparedRun> {
        let lease = self.authorize(auth)?;
        let identity = context.identity();
        let agent_id = context.session.agent_id.clone();
        let harness = self.harness(&agent_id)?;
        let coordinator = coordinator?;
        let ownership = self.session_owner.pin(&identity.session_id)?;
        if matches!(admission, super::session_gate::RunAdmission::Continuation) {
            self.session_gate
                .confirm_orphan_cleanup(&agent_id, &identity, coordinator, || {
                    !self.session_owner.holds_turn(&identity)
                })?;
        }
        let expected = self.durable_state(&context.session).await?.history_cursor;
        let reservation = self.session_gate.reserve(
            &context.session,
            &identity,
            coordinator,
            &expected,
            admission,
        )?;
        self.authorize_admission(&lease)?;
        Ok(PreparedRun {
            lease,
            harness,
            auth: auth.clone(),
            session: context.session.clone(),
            identity: talos_types::AgentCaller {
                agent_id,
                session_id: identity.session_id,
                turn_id: identity.turn_id,
            },
            ownership,
            reservation,
        })
    }

    async fn finish_admission(
        &self,
        prepared: PreparedRun,
        execution: AgentRunExecution,
        callback: Option<&pb::CoordinatorCallContext>,
    ) -> TalosResult<AgentRunExecution> {
        let resource_guard = self
            .bind_resources(&prepared.auth, &prepared.session, callback)
            .await?;
        Ok(self.guard_run(
            prepared.lease,
            prepared.harness,
            execution,
            prepared.session,
            prepared.identity,
            (prepared.ownership, resource_guard, prepared.reservation),
        ))
    }

    async fn replay_active(
        &self,
        auth: &NodeAuth,
        params: &agent_protocol::AgentRunParams,
        coordinator: &str,
    ) -> TalosResult<Option<AgentRunExecution>> {
        self.authorize(auth)?;
        let marker = self.session_gate.read(&params.context.session)?;
        if marker.active_turn.as_ref() != Some(&params.context.identity())
            || marker.coordinator_id.as_deref() != Some(coordinator)
        {
            return Ok(None);
        }
        let harness = self.harness(&params.context.session.agent_id)?;
        let state = harness
            .read_session_execution_state(params.context.session.clone())
            .await?;
        if !state
            .is_some_and(|state| state.active_turn.as_ref() == Some(&params.context.identity()))
        {
            return Ok(None);
        }
        // Query the journal without admitting work: a new continuation shares
        // the Turn identity but must reacquire the original protected resources.
        harness.replay_admitted_run(params.clone()).await
    }

    pub(super) async fn admit_start(
        &self,
        auth: &NodeAuth,
        params: agent_protocol::AgentRunParams,
        coordinator: &str,
    ) -> TalosResult<AgentRunExecution> {
        if let Some(execution) = self.replay_active(auth, &params, coordinator).await? {
            return Ok(execution);
        }
        let admission = if params.intent == agent_protocol::AgentRunIntent::RespondUserAction {
            super::session_gate::RunAdmission::Continuation
        } else {
            super::session_gate::RunAdmission::NewTurn
        };
        let prepared = self
            .prepare_run(auth, &params.context, admission, Ok(coordinator))
            .await?;
        let execution = prepared.harness.start(params).await?;
        self.finish_admission(prepared, execution, None).await
    }

    pub(super) async fn serve_start(
        &self,
        request: RpcStream<pb::StartRequest>,
    ) -> Result<Response<RpcStream<pb::RunResponse>>, Status> {
        let mut inbound = request;
        let first = inbound.next().await.transpose()?.ok_or_else(|| {
            status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing Agent request",
            ))
        })?;
        let auth = node_pb::decode_auth(first.auth).map_err(status_from_error)?;
        let lease = self.authorize(&auth).map_err(status_from_error)?;
        let Some(pb::start_params::Body::Run(params)) = first.params.and_then(|value| value.body)
        else {
            return Err(status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing run params",
            )));
        };
        let params = harness_pb::decode_run_params(
            *params,
            &self.config.read().expect("Worker config lock poisoned"),
        )
        .map_err(status_from_error)?;
        let admission = if params.intent == agent_protocol::AgentRunIntent::RespondUserAction {
            super::session_gate::RunAdmission::Continuation
        } else {
            super::session_gate::RunAdmission::NewTurn
        };
        let coordinator = self.coordinator_id(first.caller.as_ref(), &params.context.session);
        let coordinator = coordinator.map_err(status_from_error)?;
        if let Some(execution) = self
            .replay_active(&auth, &params, coordinator)
            .await
            .map_err(status_from_error)?
        {
            let commit = Box::pin(read_commit(
                inbound,
                start_commit,
                self.clone(),
                lease.clone(),
            ));
            return Ok(self.serve_run(commit, lease, Ok(execution)));
        }
        let prepared = self
            .prepare_run(&auth, &params.context, admission, Ok(coordinator))
            .await
            .map_err(status_from_error)?;
        let execution = match prepared.harness.start(params).await {
            Ok(execution) => Ok(self
                .finish_admission(prepared, execution, first.caller.as_ref())
                .await
                .map_err(status_from_error)?),
            Err(error) => Err(error),
        };
        let commit = Box::pin(read_commit(
            inbound,
            start_commit,
            self.clone(),
            lease.clone(),
        ));
        Ok(self.serve_run(commit, lease, execution))
    }

    pub(super) async fn admit_compact(
        &self,
        auth: &NodeAuth,
        context: agent_protocol::AgentContext,
        coordinator: &str,
    ) -> TalosResult<AgentRunExecution> {
        let prepared = self
            .prepare_run(
                auth,
                &context,
                super::session_gate::RunAdmission::NewTurn,
                Ok(coordinator),
            )
            .await?;
        let execution = prepared.harness.compact_session(context).await?;
        self.finish_admission(prepared, execution, None).await
    }

    pub(super) async fn serve_compact_session(
        &self,
        request: RpcStream<pb::CompactSessionRequest>,
    ) -> Result<Response<RpcStream<pb::RunResponse>>, Status> {
        let mut inbound = request;
        let first = inbound.next().await.transpose()?.ok_or_else(|| {
            status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing Agent request",
            ))
        })?;
        let auth = node_pb::decode_auth(first.auth).map_err(status_from_error)?;
        let lease = self.authorize(&auth).map_err(status_from_error)?;
        let Some(pb::compact_session_params::Body::Context(context)) =
            first.params.and_then(|value| value.body)
        else {
            return Err(status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing compaction context",
            )));
        };
        let context = harness_pb::decode_context(
            Some(*context),
            &self.config.read().expect("Worker config lock poisoned"),
        )
        .map_err(status_from_error)?;
        let coordinator = self.coordinator_id(first.caller.as_ref(), &context.session);
        let prepared = self
            .prepare_run(
                &auth,
                &context,
                super::session_gate::RunAdmission::NewTurn,
                coordinator,
            )
            .await
            .map_err(status_from_error)?;
        let execution = match prepared.harness.compact_session(context).await {
            Ok(execution) => Ok(self
                .finish_admission(prepared, execution, first.caller.as_ref())
                .await
                .map_err(status_from_error)?),
            Err(error) => Err(error),
        };
        let commit = Box::pin(read_commit(
            inbound,
            compact_commit,
            self.clone(),
            lease.clone(),
        ));
        Ok(self.serve_run(commit, lease, execution))
    }

    fn prepare_input(
        &self,
        auth: &NodeAuth,
        params: &agent_protocol::AgentInputParams,
        peer: Option<&str>,
    ) -> TalosResult<Arc<dyn AgentHarness>> {
        self.authorize(auth)?;
        let peer = peer.ok_or_else(invalid_request)?;
        self.session_gate
            .authorize_active(&params.agent_id, &params.identity, peer)?;
        if !self
            .active
            .lock()
            .expect("Agent active lock poisoned")
            .contains_key(&(
                params.agent_id.clone(),
                params.identity.session_id.clone(),
                params.identity.turn_id.clone(),
            ))
        {
            return Err(TalosError::new(
                ErrorCode::ResourceNotFound,
                "Agent Turn is not active on this node",
            ));
        }
        self.harness(&params.agent_id)
    }

    pub(super) async fn admit_input(
        &self,
        auth: &NodeAuth,
        params: agent_protocol::AgentInputParams,
        peer: &str,
    ) -> TalosResult<agent_protocol::AgentAdmission> {
        self.prepare_input(auth, &params, Some(peer))?
            .submit_input(params)
            .await
    }

    pub(super) async fn serve_submit_input(
        &self,
        request: RpcStream<pb::SubmitInputRequest>,
    ) -> Result<Response<RpcStream<pb::InputResponse>>, Status> {
        let mut inbound = request;
        let first = inbound.next().await.transpose()?.ok_or_else(|| {
            status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing Agent request",
            ))
        })?;
        let auth = node_pb::decode_auth(first.auth).map_err(status_from_error)?;
        let lease = self.authorize(&auth).map_err(status_from_error)?;
        let Some(pb::submit_input_params::Body::Input(params)) =
            first.params.and_then(|value| value.body)
        else {
            return Err(status_from_error(TalosError::new(
                ErrorCode::InvalidRequest,
                "missing input params",
            )));
        };
        let params = harness_pb::decode_input_params(*params).map_err(status_from_error)?;
        let peer = first
            .caller
            .as_ref()
            .map(|caller| caller.coordinator_id.as_str());
        let harness = self
            .prepare_input(&auth, &params, peer)
            .map_err(status_from_error)?;
        let admission = harness.submit_input(params).await;
        let (tx, rx) = mpsc::channel(4);
        let adapter = self.clone();
        tokio::spawn(async move {
            let admission = match admission {
                Ok(value) => value,
                Err(error) => {
                    let _ = tx.send(Ok(input_failure(error))).await;
                    return;
                }
            };
            let events = match harness_pb::encode_admission_events(admission.events) {
                Ok(value) => value,
                Err(error) => {
                    let _ = tx.send(Ok(input_failure(error))).await;
                    return;
                }
            };
            if tx
                .send(Ok(pb::InputResponse {
                    result: Some(pb::input_response::Result::Admission(events)),
                }))
                .await
                .is_err()
            {
                return;
            }
            let commit = read_commit(inbound, input_commit, adapter, lease.clone()).await;
            if let Err(error) = commit {
                let _ = tx.send(Ok(input_failure(error))).await;
                return;
            }
            let accepted = tokio::select! {
                biased;
                _ = lease.ended() => return,
                _ = tx.closed() => return,
                result = admission.permit.consume() => result,
            };
            let frame = match accepted {
                Ok(()) => pb::InputResponse {
                    result: Some(pb::input_response::Result::Accepted(pb::Empty {})),
                },
                Err(error) => input_failure(error),
            };
            let _ = tx.send(Ok(frame)).await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}

fn confirms_cleanup(settlement: &agent_protocol::AgentRunSettlement) -> bool {
    !matches!(
        settlement,
        agent_protocol::AgentRunSettlement::Finished { result: Err(error), .. }
            if error.is_code(ErrorCode::RunCleanupUnconfirmed)
    )
}
