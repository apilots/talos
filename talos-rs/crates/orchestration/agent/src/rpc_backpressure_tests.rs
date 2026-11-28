use super::*;
use agent_protocol::{
    AgentAdmission, AgentAdmissionPermit, AgentContext, AgentFuture, AgentInputParams,
    AgentProgressEventReceiver, AgentRunParams, AgentRunSettlementReceiver, AgentSessionContext,
};
use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentInterruptOutcome, AgentRunMetrics, AgentTurnRef,
};
use tokio::sync::oneshot;

struct RunningHarness(
    CancellationToken,
    mpsc::Sender<AgentEvent>,
    TalosResult<AgentInterruptOutcome>,
);

impl AgentHarness for RunningHarness {
    fn start(&self, _: AgentRunParams) -> AgentFuture<'_, TalosResult<AgentRunExecution>> {
        unreachable!()
    }
    fn submit_input(&self, _: AgentInputParams) -> AgentFuture<'_, TalosResult<AgentAdmission>> {
        unreachable!()
    }
    fn compact_session(&self, _: AgentContext) -> AgentFuture<'_, TalosResult<AgentRunExecution>> {
        unreachable!()
    }
    fn clear_session(
        &self,
        _: AgentSessionContext,
        _expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, TalosResult<()>> {
        unreachable!()
    }
    fn close_session(
        &self,
        _: AgentSessionContext,
        _expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> AgentFuture<'_, TalosResult<()>> {
        unreachable!()
    }
    fn shutdown(&self, _: Duration) -> AgentFuture<'_, TalosResult<()>> {
        unreachable!()
    }
    fn interrupt(
        &self,
        _: AgentInterruptParams,
    ) -> AgentFuture<'_, TalosResult<AgentInterruptOutcome>> {
        Box::pin(async {
            // The real Provider can be blocked in an event send. Its consumer
            // must be closed before interruption waits for cooperative cleanup.
            self.1.closed().await;
            self.0.cancel();
            self.2.clone()
        })
    }
}

#[tokio::test]
async fn unread_progress_stream_still_interrupts_on_revocation_and_expiry() {
    for revoke in [true, false] {
        let stopped = CancellationToken::new();
        let (events, receiver) = mpsc::channel(1);
        let harness: Arc<dyn AgentHarness> = Arc::new(RunningHarness(
            stopped.clone(),
            events.clone(),
            Ok(AgentInterruptOutcome::Cancelled),
        ));
        let home = tempfile::tempdir().unwrap();
        let adapter = AgentHarnessRpcAdapter::new(
            "node".into(),
            Arc::new(std::sync::RwLock::new(
                Config::default().with_runtime_paths(talos_config::TalosPaths::from_home(
                    home.path().to_owned(),
                )),
            )),
            vec![("agent".into(), harness.clone())],
        );
        adapter
            .install_registration(
                Registration {
                    lease_token: "token".into(),
                    owner_user_id: "owner".into(),
                    validity: agent_protocol::LeaseValidity {
                        server_at: 0,
                        expires_at: 90,
                    },
                },
                Instant::now() + Duration::from_secs(90),
            )
            .unwrap();
        let lease = adapter
            .authorize(&NodeAuth {
                node_id: "node".into(),
                token: "token".into(),
            })
            .unwrap();
        let identity = AgentTurnRef::new("session", "turn");
        let (_settle, settlement) = oneshot::channel();
        let (accepted, did_accept) = oneshot::channel();
        let execution = adapter.guard_run(
            lease.clone(),
            harness,
            AgentRunExecution {
                admission: AgentAdmission {
                    events: vec![],
                    permit: AgentAdmissionPermit::new(async move {
                        accepted.send(()).unwrap();
                        Ok(())
                    }),
                },
                events: AgentProgressEventReceiver::new(receiver),
                settlement: AgentRunSettlementReceiver::new(settlement),
            },
            AgentSessionContext {
                agent_id: "agent".into(),
                session_id: "session".into(),
                workspace: adapter
                    .config
                    .read()
                    .unwrap()
                    .resolve_workspace("default", "owner")
                    .unwrap(),
            },
            talos_types::AgentCaller {
                session_id: identity.session_id.clone(),
                turn_id: identity.turn_id.clone(),
                agent_id: "agent".into(),
            },
            (
                adapter.session_owner.pin("session").unwrap(),
                Box::new(()) as Box<dyn Send>,
                adapter
                    .session_gate
                    .reserve(
                        &agent_protocol::AgentSessionContext {
                            agent_id: "agent".into(),
                            session_id: "session".into(),
                            workspace: adapter
                                .config
                                .read()
                                .unwrap()
                                .resolve_workspace("default", "owner")
                                .unwrap(),
                        },
                        &identity,
                        &uuid::Uuid::new_v4().to_string(),
                        &Default::default(),
                        super::session_gate::RunAdmission::NewTurn,
                    )
                    .unwrap(),
            ),
        );
        let response = adapter.serve_run(Box::pin(async { Ok(()) }), lease.clone(), Ok(execution));
        did_accept.await.unwrap();
        // Keep the response alive without reading until a progress send is
        // actually pending, so lease revocation is tested against backpressure
        // rather than a still-draining queue.
        let mut sequence = 0;
        let blocked;
        loop {
            sequence += 1;
            assert!(
                sequence <= 64,
                "progress should block within the existing RPC frame queue"
            );
            let mut send = Box::pin(events.send(AgentEvent {
                identity: identity.clone(),
                sequence,
                occurred_at: 0,
                payload: AgentEventPayload::MetricsUpdated(AgentRunMetrics::default()),
            }));
            match tokio::time::timeout(Duration::from_millis(200), send.as_mut()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => panic!("progress send failed before the lease ended"),
                Err(_) => {
                    blocked = Some(send);
                    break;
                }
            }
        }
        let _blocked = blocked;
        assert_eq!(events.capacity(), 0);
        assert!(!stopped.is_cancelled());
        if revoke {
            adapter.clear_registration();
        } else {
            lease.deadline.send_replace(Instant::now());
        }
        tokio::time::timeout(Duration::from_secs(2), stopped.cancelled())
            .await
            .expect("lease must interrupt even while nobody reads");
        tokio::time::timeout(Duration::from_secs(2), events.closed())
            .await
            .unwrap();
        assert!(adapter.active.lock().unwrap().is_empty());
        // Cancellation never fabricates a successful settlement.
        let mut stream = response.into_inner();
        use tokio_stream::StreamExt;
        while let Some(frame) = stream.next().await {
            assert!(!matches!(
                frame.unwrap().result,
                Some(pb::run_response::Result::Settlement(_))
            ));
        }
    }
}

#[tokio::test]
async fn disconnected_execution_continues_until_authorization_revocation() {
    assert_disconnected_cleanup(false).await;
    assert_disconnected_cleanup(true).await;
}

async fn assert_disconnected_cleanup(finished: bool) {
    let (events, receiver) = mpsc::channel(1);
    let stopped = CancellationToken::new();
    let harness: Arc<dyn AgentHarness> = Arc::new(RunningHarness(
        stopped.clone(),
        events,
        if finished {
            Err(TalosError::new(
                ErrorCode::ResourceNotFound,
                "finished Harness Turn was removed",
            ))
        } else {
            Ok(AgentInterruptOutcome::CleanupUnconfirmed { task_count: 1 })
        },
    ));
    let home = tempfile::tempdir().unwrap();
    let adapter = AgentHarnessRpcAdapter::new(
        "node".into(),
        Arc::new(std::sync::RwLock::new(
            Config::default()
                .with_runtime_paths(talos_config::TalosPaths::from_home(home.path().to_owned())),
        )),
        vec![("agent".into(), harness.clone())],
    );
    adapter
        .install_registration(
            Registration {
                lease_token: "token".into(),
                owner_user_id: "owner".into(),
                validity: agent_protocol::LeaseValidity {
                    server_at: 0,
                    expires_at: 90,
                },
            },
            Instant::now() + Duration::from_secs(90),
        )
        .unwrap();
    let lease = adapter
        .authorize(&NodeAuth {
            node_id: "node".into(),
            token: "token".into(),
        })
        .unwrap();
    let (settle, settlement) = oneshot::channel();
    let identity = AgentTurnRef::new("session", "turn");
    let peer = uuid::Uuid::new_v4().to_string();
    let (accepted, did_accept) = oneshot::channel();
    let execution = adapter.guard_run(
        lease.clone(),
        harness,
        AgentRunExecution {
            admission: AgentAdmission {
                events: vec![],
                permit: AgentAdmissionPermit::new(async move {
                    accepted.send(()).unwrap();
                    Ok(())
                }),
            },
            events: AgentProgressEventReceiver::new(receiver),
            settlement: AgentRunSettlementReceiver::new(settlement),
        },
        agent_protocol::AgentSessionContext {
            agent_id: "agent".into(),
            session_id: "session".into(),
            workspace: adapter
                .config
                .read()
                .unwrap()
                .resolve_workspace("default", "owner")
                .unwrap(),
        },
        talos_types::AgentCaller {
            session_id: "session".into(),
            turn_id: "turn".into(),
            agent_id: "agent".into(),
        },
        (
            adapter.session_owner.pin("session").unwrap(),
            Box::new(()) as Box<dyn Send>,
            adapter
                .session_gate
                .reserve(
                    &agent_protocol::AgentSessionContext {
                        agent_id: "agent".into(),
                        session_id: "session".into(),
                        workspace: adapter
                            .config
                            .read()
                            .unwrap()
                            .resolve_workspace("default", "owner")
                            .unwrap(),
                    },
                    &identity,
                    &peer,
                    &Default::default(),
                    super::session_gate::RunAdmission::NewTurn,
                )
                .unwrap(),
        ),
    );
    let response = adapter.serve_run(Box::pin(async { Ok(()) }), lease.clone(), Ok(execution));
    did_accept.await.unwrap();
    if finished {
        settle
            .send(Ok(agent_protocol::AgentRunSettlement::Finished {
                event: AgentEvent {
                    identity: identity.clone(),
                    sequence: 2,
                    occurred_at: 1,
                    payload: AgentEventPayload::TurnCompleted(
                        talos_agent_api::AgentTurnCompleted {
                            stop_reason: talos_agent_api::AgentStopReason::EndTurn,
                        },
                    ),
                },
                metrics: AgentRunMetrics::default(),
                result: Ok(()),
            }))
            .unwrap();
    }
    drop(response);
    tokio::task::yield_now().await;
    assert!(!stopped.is_cancelled());
    adapter.clear_registration();
    let cleanup = tokio::time::timeout(Duration::from_secs(2), adapter.drain_disconnected())
        .await
        .unwrap();
    assert!(stopped.is_cancelled());
    if finished {
        cleanup.unwrap();
        assert!(adapter
            .session_gate
            .authorize_active("agent", &identity, &peer)
            .unwrap());
    } else {
        assert!(cleanup
            .unwrap_err()
            .is_code(ErrorCode::RunCleanupUnconfirmed));
        assert!(!adapter
            .session_gate
            .authorize_active("agent", &identity, &peer)
            .unwrap());
    }
}
