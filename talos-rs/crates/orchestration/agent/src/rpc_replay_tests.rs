use super::*;
use agent_protocol::AgentRunExecution;
use std::sync::atomic::AtomicUsize;

#[derive(Default)]
struct ResourceCounts {
    bound: AtomicUsize,
    released: AtomicUsize,
}
struct ResourceGuard(Arc<ResourceCounts>);
impl Drop for ResourceGuard {
    fn drop(&mut self) {
        self.0.released.fetch_add(1, Ordering::SeqCst);
    }
}

async fn waiting(execution: AgentRunExecution) -> talos_agent_api::AgentEvent {
    let mut execution = execution;
    execution.admission.permit.consume().await.unwrap();
    while execution.events.recv().await.is_some() {}
    let result = execution.settlement.recv().await.unwrap();
    assert!(!format!("{result:?}").contains("continuation-1"));
    let encoded = prost::Message::encode_to_vec(
        &agent_protocol::harness_pb::encode_settlement(result.clone()).unwrap(),
    );
    assert!(!String::from_utf8_lossy(&encoded).contains("continuation-1"));
    match result {
        AgentRunSettlement::WaitingForInput { event, .. } => event,
        result => panic!("expected waiting settlement, got {result:?}"),
    }
}

fn counted_binder() -> (Arc<ResourceCounts>, crate::AgentInvocationResourceBinder) {
    let counts = Arc::new(ResourceCounts::default());
    let observed = counts.clone();
    let binder: crate::AgentInvocationResourceBinder = Arc::new(move |_, _, _| {
        observed.bound.fetch_add(1, Ordering::SeqCst);
        let guard = ResourceGuard(observed.clone());
        Box::pin(async move { Ok(Box::new(guard) as Box<dyn Send>) })
    });
    (counts, binder)
}

#[tokio::test]
async fn waiting_continuation_rebinds_resources_but_exact_replay_does_not() {
    let (counts, binder) = counted_binder();
    let (_home, _provider, _adapter, client) =
        library_client_with_binder(FakeMode::Waiting, Some(binder)).await;
    assert_continuation_resources(client, counts).await;
}

#[tokio::test]
async fn rpc_continuation_rebinds_resources_but_exact_replay_does_not() {
    let (_journal_directory, journal) = crate::harness::test_support::execution_store().await;
    let (counts, binder) = counted_binder();
    let (client, _adapter, _pem, _endpoint, stop, task) = server_with_binder(
        Arc::new(Harness::new(
            TEST_AGENT_ID.into(),
            FakeProvider::new(FakeMode::Waiting),
            journal.clone(),
        )),
        Some(binder),
    )
    .await;
    assert_continuation_resources(Arc::new(client), counts).await;
    stop.cancel();
    task.await.unwrap();
}

async fn assert_continuation_resources(client: Arc<dyn AgentHarness>, counts: Arc<ResourceCounts>) {
    let identity = AgentTurnRef::new("session-1", "resource-continuation");
    waiting(
        client
            .start(run_params(identity.clone(), "question"))
            .await
            .unwrap(),
    )
    .await;
    let resume =
        crate::harness::test_support::response_params(identity.clone(), "answer", "action-1");
    waiting(client.start(resume.clone()).await.unwrap()).await;
    assert_eq!(
        counts.bound.load(Ordering::SeqCst),
        2,
        "a new continuation must bind its own protected resources"
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while counts.released.load(Ordering::SeqCst) != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut conflicting = resume.clone();
    conflicting.input.created_at += 1;
    assert!(client
        .start(conflicting)
        .await
        .err()
        .unwrap()
        .is_code(ErrorCode::IdempotencyConflict));
    waiting(client.start(resume).await.unwrap()).await;
    assert_eq!(
        counts.bound.load(Ordering::SeqCst),
        2,
        "an exact committed command replay must not replace resource bindings"
    );
    assert_eq!(counts.released.load(Ordering::SeqCst), 2);
    client
        .interrupt(AgentInterruptParams {
            agent_id: TEST_AGENT_ID.into(),
            identity,
        })
        .await
        .unwrap();
    client.shutdown(Duration::from_secs(2)).await.unwrap();
}

#[tokio::test]
async fn waiting_rpc_cancellation_and_failed_continuation_keep_the_original_boundary() {
    let (_journal_directory, journal) = crate::harness::test_support::execution_store().await;
    let provider = FakeProvider::new(FakeMode::Waiting);
    let harness = Arc::new(Harness::new(
        TEST_AGENT_ID.into(),
        provider,
        journal.clone(),
    ));
    let (client, _, certificate, endpoint, stop, task) = server(harness).await;
    let peer = uuid::Uuid::new_v4().to_string();
    let client = client.with_coordinator_id(peer.clone()).unwrap();
    let identity = AgentTurnRef::new("session-1", "waiting-turn");
    let run = client
        .start(run_params(identity.clone(), "question"))
        .await
        .unwrap();
    run.admission.permit.consume().await.unwrap();
    let AgentRunSettlement::WaitingForInput { .. } = run.settlement.recv().await.unwrap() else {
        panic!("expected waiting settlement");
    };
    let before = client
        .read_session_execution_state(session_params())
        .await
        .unwrap()
        .unwrap();
    let invalid =
        crate::harness::test_support::response_params(identity.clone(), "answer", "wrong-action");
    assert!(
        matches!(client.start(invalid).await, Err(error) if error.is_code(ErrorCode::Conflict))
    );
    assert_eq!(
        client
            .read_session_execution_state(session_params())
            .await
            .unwrap()
            .unwrap(),
        before
    );
    assert!(client
        .clear_session(session_params(), Default::default())
        .await
        .unwrap_err()
        .is_code(ErrorCode::ActiveTurnRunning));
    let resume =
        crate::harness::test_support::response_params(identity.clone(), "answer", "action-1");
    // Close the actual request stream after admission without a commit frame.
    let channel = tonic::transport::Endpoint::from_shared(endpoint)
        .unwrap()
        .tls_config(
            tonic::transport::ClientTlsConfig::new()
                .ca_certificate(tonic::transport::Certificate::from_pem(certificate)),
        )
        .unwrap()
        .connect_lazy();
    let first = agent_pb::StartRequest {
        auth: Some(agent_protocol::node_pb::encode_auth(NodeAuth {
            node_id: "node-1".into(),
            token: "lease-secret".into(),
        })),
        caller: Some(agent_pb::CoordinatorCallContext {
            coordinator_id: peer,
            session: Some(agent_protocol::harness_pb::encode_session_context(
                session_params(),
            )),
            turn_id: Some(identity.turn_id.clone()),
            ..Default::default()
        }),
        params: Some(agent_pb::StartParams {
            body: Some(agent_pb::start_params::Body::Run(Box::new(
                agent_protocol::harness_pb::encode_run_params(resume).unwrap(),
            ))),
        }),
    };
    let mut unfinished = agent_pb::agent_harness_rpc_client::AgentHarnessRpcClient::new(channel)
        .start(tokio_stream::iter([first]))
        .await
        .unwrap()
        .into_inner();
    assert!(matches!(
        unfinished.message().await.unwrap().unwrap().result,
        Some(agent_pb::run_response::Result::Admission(_))
    ));
    assert!(matches!(
        unfinished.message().await.unwrap().unwrap().result,
        Some(agent_pb::run_response::Result::Failure(_))
    ));
    drop(unfinished);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match client
                .interrupt(AgentInterruptParams {
                    agent_id: TEST_AGENT_ID.into(),
                    identity: identity.clone(),
                })
                .await
            {
                Ok(AgentInterruptOutcome::Cancelled) => break,
                Err(error) if error.is_code(ErrorCode::RunCleanupUnconfirmed) => {
                    tokio::task::yield_now().await
                }
                result => panic!("waiting cleanup changed: {result:?}"),
            }
        }
    })
    .await
    .unwrap();
    let after = client
        .read_session_execution_state(session_params())
        .await
        .unwrap()
        .unwrap();
    assert!(after.active_turn.is_none());
    assert!(after.handoff_ready);
    assert_eq!(
        after.history_cursor.completed_turn_id.as_deref(),
        Some(identity.turn_id.as_str())
    );
    assert!(client
        .clear_session(session_params(), Default::default())
        .await
        .unwrap_err()
        .is_code(ErrorCode::EventCursorExpired));
    assert!(client
        .interrupt(AgentInterruptParams {
            agent_id: TEST_AGENT_ID.into(),
            identity
        })
        .await
        .unwrap_err()
        .is_code(ErrorCode::Conflict));
    stop.cancel();
    task.await.unwrap();
}

#[tokio::test]
async fn library_waiting_continuation_and_cancel_keep_the_original_boundary() {
    let (_home, _provider, _adapter, client) = library_client(FakeMode::Waiting).await;
    let identity = AgentTurnRef::new("session-1", "local-waiting");
    let mut execution = client
        .start(run_params(identity.clone(), "question"))
        .await
        .unwrap();
    execution.admission.permit.consume().await.unwrap();
    while execution.events.recv().await.is_some() {}
    let AgentRunSettlement::WaitingForInput { .. } = execution.settlement.recv().await.unwrap()
    else {
        panic!("expected waiting");
    };
    let resume =
        crate::harness::test_support::response_params(identity.clone(), "answer", "action-1");
    let mut resumed = client.start(resume).await.unwrap();
    resumed.admission.permit.consume().await.unwrap();
    while resumed.events.recv().await.is_some() {}
    assert!(matches!(
        resumed.settlement.recv().await.unwrap(),
        AgentRunSettlement::WaitingForInput { .. }
    ));
    assert_eq!(
        client
            .interrupt(AgentInterruptParams {
                agent_id: TEST_AGENT_ID.into(),
                identity
            })
            .await
            .unwrap(),
        AgentInterruptOutcome::Cancelled
    );
    client.shutdown(Duration::from_secs(2)).await.unwrap();
}
