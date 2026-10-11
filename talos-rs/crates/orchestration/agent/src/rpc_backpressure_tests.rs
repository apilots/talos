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
    AgentInterruptOutcome,
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
    fn clear_session(&self, _: AgentSessionContext) -> AgentFuture<'_, TalosResult<()>> {
        unreachable!()
    }
    fn close_session(&self, _: AgentSessionContext) -> AgentFuture<'_, TalosResult<()>> {
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
            Ok(self.2.clone())
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
            AgentInterruptOutcome::Cancelled,
        ));
        let adapter = AgentHarnessRpcAdapter::new(
            "node".into(),
            Arc::new(std::sync::RwLock::new(Config::default())),
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
        let response = adapter.serve_run(
            Box::pin(async { Ok(()) }),
            lease.clone(),
            harness,
            Ok(AgentRunExecution {
                admission: AgentAdmission {
                    events: vec![],
                    permit: AgentAdmissionPermit::new(async move {
                        accepted.send(()).unwrap();
                        Ok(())
                    }),
                },
                events: AgentProgressEventReceiver::new(receiver),
                settlement: AgentRunSettlementReceiver::new(settlement),
            }),
            identity.clone(),
            "agent".into(),
        );
        did_accept.await.unwrap();
        // Keep the response alive without reading. Admission + accepted + 30
        // progress frames fill the real adapter queue. Frame 31 blocks send.
        for sequence in 1..=32 {
            events
                .send(AgentEvent {
                    identity: identity.clone(),
                    sequence,
                    occurred_at: 0,
                    payload: AgentEventPayload::MetricsUpdated(AgentRunMetrics::default()),
                })
                .await
                .unwrap();
        }
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
async fn reconnect_remains_blocked_after_unconfirmed_disconnected_cleanup() {
    let (events, receiver) = mpsc::channel(1);
    let stopped = CancellationToken::new();
    let harness: Arc<dyn AgentHarness> = Arc::new(RunningHarness(
        stopped.clone(),
        events,
        AgentInterruptOutcome::CleanupUnconfirmed { task_count: 1 },
    ));
    let adapter = AgentHarnessRpcAdapter::new(
        "node".into(),
        Arc::new(std::sync::RwLock::new(Config::default())),
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
    let (_settle, settlement) = oneshot::channel();
    let (accepted, did_accept) = oneshot::channel();
    let response = adapter.serve_run(
        Box::pin(async { Ok(()) }),
        lease,
        harness,
        Ok(AgentRunExecution {
            admission: AgentAdmission {
                events: vec![],
                permit: AgentAdmissionPermit::new(async move {
                    accepted.send(()).unwrap();
                    Ok(())
                }),
            },
            events: AgentProgressEventReceiver::new(receiver),
            settlement: AgentRunSettlementReceiver::new(settlement),
        }),
        AgentTurnRef::new("session", "turn"),
        "agent".into(),
    );
    did_accept.await.unwrap();
    drop(response);
    let error = tokio::time::timeout(Duration::from_secs(2), adapter.drain_disconnected())
        .await
        .unwrap()
        .unwrap_err();
    assert!(stopped.is_cancelled());
    assert!(error.is_code(ErrorCode::RunCleanupUnconfirmed));
}
