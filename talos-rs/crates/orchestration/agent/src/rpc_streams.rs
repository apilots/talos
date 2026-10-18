use super::*;

impl AgentHarnessRpcAdapter {
    pub(super) async fn finish_disconnected(
        &self,
        harness: &Arc<dyn AgentHarness>,
        agent_id: &str,
        identity: &talos_agent_api::AgentTurnRef,
    ) {
        if let Err(error) = interrupt_disconnected(harness, agent_id, identity)
            .await
            .and_then(|_| self.session_gate.cleaned(identity))
        {
            talos_logging::report_error(&error);
            self.cleanup_failure
                .lock()
                .expect("Agent cleanup lock poisoned")
                .get_or_insert(error);
        }
    }

    pub(super) fn serve_run(
        &self,
        commit: CommitFuture,
        lease: Lease,
        harness: Arc<dyn AgentHarness>,
        execution: TalosResult<AgentRunExecution>,
        caller: talos_types::AgentCaller,
        resources: (Box<dyn Send>, super::session_gate::Reservation),
    ) -> Response<RpcStream<pb::RunResponse>> {
        let (resource_guard, mut reservation) = resources;
        let identity = caller.turn_ref();
        let agent_id = caller.agent_id;
        let (tx, rx) = mpsc::channel(32);
        let adapter = self.clone();
        tokio::spawn(async move {
            let _resources = resource_guard;
            let mut execution = match execution {
                Ok(value) => value,
                Err(error) => {
                    let _ = lease.send(&tx, Ok(run_failure(error))).await;
                    return;
                }
            };
            let admission = match harness_pb::encode_admission_events(execution.admission.events) {
                Ok(value) => value,
                Err(error) => {
                    let _ = lease.send(&tx, Ok(run_failure(error))).await;
                    return;
                }
            };
            if lease
                .send(
                    &tx,
                    Ok(pb::RunResponse {
                        result: Some(pb::run_response::Result::Admission(admission)),
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
            if lease.revoked.is_cancelled() || lease.expired() {
                return;
            }
            if let Err(error) = reservation.consuming(&identity) {
                let _ = lease.send(&tx, Ok(run_failure(error))).await;
                return;
            }
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
            let consumed = tokio::select! {
                biased;
                _ = lease.ended() => None,
                _ = tx.closed() => None,
                result = execution.admission.permit.consume() => Some(result),
            };
            match consumed {
                Some(Ok(())) => {}
                Some(Err(error)) => {
                    if error.is_code(ErrorCode::RunCleanupUnconfirmed) {
                        adapter
                            .cleanup_failure
                            .lock()
                            .expect("Agent cleanup lock poisoned")
                            .get_or_insert(error.clone());
                    }
                    let _ = lease.send(&tx, Ok(run_failure(error))).await;
                    adapter
                        .active
                        .lock()
                        .expect("Agent active lock poisoned")
                        .remove(&(agent_id, identity.session_id, identity.turn_id));
                    return;
                }
                None => {
                    drop(execution.events);
                    adapter
                        .finish_disconnected(&harness, &agent_id, &identity)
                        .await;
                    adapter
                        .active
                        .lock()
                        .expect("Agent active lock poisoned")
                        .remove(&(agent_id, identity.session_id, identity.turn_id));
                    return;
                }
            }
            if lease
                .send(
                    &tx,
                    Ok(pb::RunResponse {
                        result: Some(pb::run_response::Result::Accepted(pb::Empty {})),
                    }),
                )
                .await
                .is_err()
            {
                drop(execution.events);
                adapter
                    .finish_disconnected(&harness, &agent_id, &identity)
                    .await;
                adapter
                    .active
                    .lock()
                    .expect("Agent active lock poisoned")
                    .remove(&(agent_id, identity.session_id, identity.turn_id));
                return;
            }
            let mut settlement = Box::pin(execution.settlement.recv());
            let mut events_open = true;
            loop {
                tokio::select! {
                    biased;
                    _ = lease.ended() => break,
                    _ = tx.closed() => break,
                    event = execution.events.recv(), if events_open => {
                        if let Some(event) = event {
                            let frame = harness_pb::encode_event(event).map(|value| pb::RunResponse { result: Some(pb::run_response::Result::Progress(value)) });
                            match frame { Ok(frame) => { if lease.send(&tx, Ok(frame)).await.is_err() { break; } }, Err(error) => { let _ = lease.send(&tx, Ok(run_failure(error))).await; break; } }
                        } else { events_open = false; }
                    }
                    result = &mut settlement, if !events_open => {
                        let result = result.and_then(|settlement| {
                            if !matches!(&settlement, agent_protocol::AgentRunSettlement::Finished { result: Err(error), .. } if error.is_code(ErrorCode::RunCleanupUnconfirmed)) {
                                adapter.session_gate.cleaned(&identity)?;
                            }
                            Ok(settlement)
                        });
                        let frame = match result.and_then(|settlement| {
                            if let agent_protocol::AgentRunSettlement::Finished { result: Err(error), .. } = &settlement { talos_logging::report_error(error); }
                            harness_pb::encode_settlement(settlement)
                        }) {
                            Ok(value) => pb::RunResponse { result: Some(pb::run_response::Result::Settlement(value)) },
                            Err(error) => run_failure(error),
                        };
                        adapter.active.lock().expect("Agent active lock poisoned").remove(&(agent_id, identity.session_id, identity.turn_id));
                        drop(reservation);
                        let _ = lease.send(&tx, Ok(frame)).await;
                        return;
                    }
                }
            }
            // A disconnected client or revoked lease is never a confirmed
            // business result. The existing Harness owns cancellation cleanup.
            drop(execution.events);
            adapter
                .finish_disconnected(&harness, &agent_id, &identity)
                .await;
            adapter
                .active
                .lock()
                .expect("Agent active lock poisoned")
                .remove(&(agent_id, identity.session_id, identity.turn_id));
        });
        Response::new(Box::pin(ReceiverStream::new(rx)))
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
            params,
            &self.config.read().expect("Worker config lock poisoned"),
        )
        .map_err(status_from_error)?;
        let identity = params.context.identity();
        let agent_id = params.context.session.agent_id.clone();
        let harness = self.harness(&agent_id).map_err(status_from_error)?;
        let session_context = params.context.session.clone();
        let coordinator = self
            .coordinator_id(first.caller.as_ref(), &session_context)
            .map_err(status_from_error)?;
        let reservation = self
            .session_gate
            .reserve(
                &session_context,
                &identity,
                coordinator,
                &params.context.history_cursor.clone().unwrap_or_default(),
                if params.continuation.is_some() {
                    super::session_gate::RunAdmission::Continuation
                } else {
                    super::session_gate::RunAdmission::NewTurn
                },
            )
            .map_err(status_from_error)?;
        self.authorize_admission(&lease)
            .map_err(status_from_error)?;
        let execution = harness.start(params).await;
        let resource_guard = if execution.is_ok() {
            self.bind_resources(&auth, &session_context, first.caller.as_ref())
                .map_err(status_from_error)?
        } else {
            Box::new(())
        };
        let commit = Box::pin(read_commit(
            inbound,
            start_commit,
            self.clone(),
            lease.clone(),
        ));
        Ok(self.serve_run(
            commit,
            lease,
            harness,
            execution,
            talos_types::AgentCaller {
                session_id: identity.session_id,
                turn_id: identity.turn_id,
                agent_id,
            },
            (resource_guard, reservation),
        ))
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
            Some(context),
            &self.config.read().expect("Worker config lock poisoned"),
        )
        .map_err(status_from_error)?;
        let identity = context.identity();
        let agent_id = context.session.agent_id.clone();
        let harness = self.harness(&agent_id).map_err(status_from_error)?;
        let session_context = context.session.clone();
        let coordinator = self
            .coordinator_id(first.caller.as_ref(), &session_context)
            .map_err(status_from_error)?;
        let reservation = self
            .session_gate
            .reserve(
                &session_context,
                &identity,
                coordinator,
                &context.history_cursor.clone().unwrap_or_default(),
                super::session_gate::RunAdmission::NewTurn,
            )
            .map_err(status_from_error)?;
        self.authorize_admission(&lease)
            .map_err(status_from_error)?;
        let execution = harness.compact_session(context).await;
        let resource_guard = if execution.is_ok() {
            self.bind_resources(&auth, &session_context, first.caller.as_ref())
                .map_err(status_from_error)?
        } else {
            Box::new(())
        };
        let commit = Box::pin(read_commit(
            inbound,
            compact_commit,
            self.clone(),
            lease.clone(),
        ));
        Ok(self.serve_run(
            commit,
            lease,
            harness,
            execution,
            talos_types::AgentCaller {
                session_id: identity.session_id,
                turn_id: identity.turn_id,
                agent_id,
            },
            (resource_guard, reservation),
        ))
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
        let params = harness_pb::decode_input_params(params).map_err(status_from_error)?;
        let peer = first
            .caller
            .as_ref()
            .map(|caller| caller.coordinator_id.as_str())
            .ok_or_else(|| status_from_error(invalid_request()))?;
        self.session_gate
            .authorize_active(&params.agent_id, &params.identity, peer)
            .map_err(status_from_error)?;
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
            return Err(status_from_error(TalosError::new(
                ErrorCode::ResourceNotFound,
                "Agent Turn is not active on this node",
            )));
        }
        let harness = self.harness(&params.agent_id).map_err(status_from_error)?;
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
