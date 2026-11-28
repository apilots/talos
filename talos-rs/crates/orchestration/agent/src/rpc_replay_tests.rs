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

async fn waiting(execution: AgentRunExecution) -> talos_agent_api::AgentContinuationRef {
    let mut execution = execution;
    execution.admission.permit.consume().await.unwrap();
    while execution.events.recv().await.is_some() {}
    match execution.settlement.recv().await.unwrap() {
        AgentRunSettlement::WaitingForInput { continuation, .. } => continuation,
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
    let (counts, binder) = counted_binder();
    let (client, _adapter, _pem, _endpoint, stop, task) = server_with_binder(
        Arc::new(Harness::new(
            TEST_AGENT_ID.into(),
            FakeProvider::new(FakeMode::Waiting),
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
    let continuation = waiting(
        client
            .start(run_params(identity.clone(), "question"))
            .await
            .unwrap(),
    )
    .await;
    let mut resume = run_params(identity.clone(), "answer");
    resume.continuation = Some(continuation);
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
