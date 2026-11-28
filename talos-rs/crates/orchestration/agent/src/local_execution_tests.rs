use super::test_support::{run_params, FakeMode, FakeProvider};
use super::Harness;
use crate::{AgentHarness, LocalExecutionHarness};
use agent_protocol::{AgentCommandIdentity, AgentSessionEventsParams};
use std::sync::Arc;
use talos_agent_api::{AgentEventPayload, AgentTurnRef};
use talos_error::ErrorCode;
use talos_store::execution::WorkerExecutionStore;

async fn fixture(
    mode: FakeMode,
) -> (
    tempfile::TempDir,
    Arc<WorkerExecutionStore>,
    LocalExecutionHarness,
    FakeProvider,
) {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(
        WorkerExecutionStore::open(&directory.path().join("worker.sqlite"))
            .await
            .unwrap(),
    );
    let provider = FakeProvider::new(mode);
    let harness = LocalExecutionHarness::new(
        Arc::new(Harness::new(
            "agent-1".into(),
            provider.clone(),
            store.clone(),
        )),
        store.clone(),
    );
    (directory, store, harness, provider)
}

#[tokio::test]
async fn admission_is_durable_before_provider_effects_and_receiver_disconnect_does_not_cancel() {
    let (_directory, store, harness, provider) = fixture(FakeMode::Release).await;
    let params = run_params(AgentTurnRef::new("session-1", "turn-1"), "message-1");
    let scope = params.context.session.clone();
    let execution = harness.start(params).await.unwrap();
    assert!(store.execution_state(&scope).await.unwrap().is_none());
    execution.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    drop(execution.events);
    drop(execution.settlement);
    provider.release.notify_one();
    let page = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let page = store
                .read_events(AgentSessionEventsParams {
                    session: scope.clone(),
                    after_cursor: None,
                    limit: 256,
                })
                .await
                .unwrap();
            if page
                .events
                .last()
                .is_some_and(|fact| fact.event.payload.is_terminal())
            {
                break page;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        page.events.last().unwrap().event.payload,
        AgentEventPayload::TurnCompleted(_)
    ));
    assert!(page.active_turn.is_none());
}

#[tokio::test]
async fn same_command_replays_local_terminal_but_changed_payload_conflicts() {
    let (_directory, store, harness, provider) = fixture(FakeMode::Release).await;
    let mut params = run_params(AgentTurnRef::new("session-1", "turn-1"), "message-1");
    params.command = Some(AgentCommandIdentity {
        command: talos_types::IdempotentCommand::TurnSubmission,
        idempotency_key: "stable".into(),
        request_id: "request-1".into(),
    });
    let execution = harness.start(params.clone()).await.unwrap();
    execution.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    provider.release.notify_one();
    drop(execution.events);
    execution.settlement.recv().await.unwrap();
    let replay = harness.start(params.clone()).await.unwrap();
    replay.admission.permit.consume().await.unwrap();
    drop(replay.events);
    let settlement = replay.settlement.recv().await.unwrap();
    assert!(matches!(
        settlement,
        agent_protocol::AgentRunSettlement::Finished { result: Ok(()), .. }
    ));
    params.input.created_at += 1;
    let failure = match harness.start(params).await {
        Ok(_) => panic!("changed input was accepted"),
        Err(error) => error,
    };
    assert!(failure.is_code(ErrorCode::IdempotencyConflict));
    assert_eq!(
        store
            .terminal_events("session-1", None, 256)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn cleared_generation_rejects_old_command_and_retired_session_never_reopens() {
    let (_directory, store, harness, provider) = fixture(FakeMode::Release).await;
    let params = run_params(AgentTurnRef::new("session-1", "turn-1"), "message-1");
    let scope = params.context.session.clone();
    let execution = harness.start(params.clone()).await.unwrap();
    execution.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    provider.release.notify_one();
    drop(execution.events);
    execution.settlement.recv().await.unwrap();
    let before = store
        .execution_state(&scope)
        .await
        .unwrap()
        .unwrap()
        .history_cursor;
    harness
        .clear_session(scope.clone(), before.clone())
        .await
        .unwrap();
    harness.clear_session(scope.clone(), before).await.unwrap();
    assert_eq!(
        store
            .execution_state(&scope)
            .await
            .unwrap()
            .unwrap()
            .history_cursor
            .history_generation,
        1
    );
    let failure = match harness.start(params.clone()).await {
        Ok(_) => panic!("old generation accepted"),
        Err(error) => error,
    };
    assert!(failure.is_code(ErrorCode::EventCursorExpired));
    let current = store
        .execution_state(&scope)
        .await
        .unwrap()
        .unwrap()
        .history_cursor;
    harness
        .retire_session(scope.clone(), current)
        .await
        .unwrap();
    let failure = match harness.start(params).await {
        Ok(_) => panic!("retired session accepted"),
        Err(error) => error,
    };
    assert!(failure.is_code(ErrorCode::SessionNotFound));
    assert!(store.is_retired(&scope.session_id).await.unwrap());
}

#[tokio::test]
async fn waiting_interrupt_and_local_memory_usage_settle_at_the_worker() {
    let (_directory, store, harness, _provider) = fixture(FakeMode::Waiting).await;
    let params = run_params(
        AgentTurnRef::new("session-1", "turn-waiting"),
        "message-waiting",
    );
    let scope = params.context.session.clone();
    let identity = params.context.identity();
    let execution = harness.start(params).await.unwrap();
    execution.admission.permit.consume().await.unwrap();
    drop(execution.events);
    assert!(matches!(
        execution.settlement.recv().await.unwrap(),
        agent_protocol::AgentRunSettlement::WaitingForInput { .. }
    ));
    harness
        .interrupt(agent_protocol::AgentInterruptParams {
            agent_id: "agent-1".into(),
            identity: identity.clone(),
        })
        .await
        .unwrap();
    let (event, _) = store.settlement(&identity).await.unwrap().unwrap();
    assert!(
        matches!(event.payload,AgentEventPayload::TurnFailed(failed) if failed.error.code==ErrorCode::Cancelled)
    );
    assert!(store
        .execution_state(&scope)
        .await
        .unwrap()
        .unwrap()
        .active_turn
        .is_none());
}

#[tokio::test]
async fn local_memory_observer_usage_is_committed_with_the_original_terminal_fact() {
    let (_directory, store, harness, provider) = fixture(FakeMode::Release).await;
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let was_observed = observed.clone();
    let journal = store.clone();
    let harness = harness.with_terminal_observer(Arc::new(move |context, event| {
        let observed = was_observed.clone();
        let journal = journal.clone();
        Box::pin(async move {
            assert!(journal.settlement(&event.identity).await.unwrap().is_none());
            assert!(journal.checkpoint(&event.identity).await.unwrap().is_some());
            observed.store(true, std::sync::atomic::Ordering::SeqCst);
            vec![agent_protocol::AgentMemoryModelUsage {
                request_id: "memory-call-1".into(),
                model_ref: context.model_selection.model_ref,
                usage: talos_types::TokenUsage {
                    input_tokens: 2,
                    output_tokens: 1,
                    total_tokens: 3,
                    ..Default::default()
                },
                completed_at: 1,
                timezone: "UTC".into(),
            }]
        })
    }));
    let params = run_params(
        AgentTurnRef::new("session-1", "turn-memory"),
        "message-memory",
    );
    let identity = params.context.identity();
    let execution = harness.start(params).await.unwrap();
    execution.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    provider.release.notify_one();
    drop(execution.events);
    execution.settlement.recv().await.unwrap();
    let (_, settlement) = store.settlement(&identity).await.unwrap().unwrap();
    assert!(observed.load(std::sync::atomic::Ordering::SeqCst));
    assert!(
        matches!(settlement,agent_protocol::AgentExecutionSettlement::Finished {memory_usage,..} if memory_usage.len()==1 && memory_usage[0].request_id=="memory-call-1")
    );
}

#[tokio::test]
async fn restarted_worker_restores_waiting_input_from_its_own_journal() {
    let (directory, store, harness, _) = fixture(FakeMode::Waiting).await;
    let identity = AgentTurnRef::new("session-1", "turn-waiting");
    let params = run_params(identity.clone(), "message-waiting");
    let scope = params.context.session.clone();
    let execution = harness.start(params).await.unwrap();
    execution.admission.permit.consume().await.unwrap();
    drop(execution.events);
    let original = execution.settlement.recv().await.unwrap();
    let agent_protocol::AgentRunSettlement::WaitingForInput { event, .. } = original else {
        panic!("expected waiting Turn");
    };
    let last_sequence = event.sequence;
    drop(harness);
    drop(store);

    let store = Arc::new(
        WorkerExecutionStore::open(&directory.path().join("worker.sqlite"))
            .await
            .unwrap(),
    );
    let provider = FakeProvider::new(FakeMode::Waiting);
    let harness = LocalExecutionHarness::new(
        Arc::new(Harness::new(
            "agent-1".into(),
            provider.clone(),
            store.clone(),
        )),
        store.clone(),
    );
    let response = super::test_support::response_params(identity.clone(), "answer-1", "action-1");
    let execution = harness.start(response.clone()).await.unwrap();
    assert_eq!(execution.admission.events[0].sequence, last_sequence + 1);
    execution.admission.permit.consume().await.unwrap();
    drop(execution.events);
    let resumed = execution.settlement.recv().await.unwrap();
    assert!(matches!(
        resumed,
        agent_protocol::AgentRunSettlement::WaitingForInput { .. }
    ));
    assert_eq!(
        *provider.continuations.lock().unwrap(),
        vec![Some(talos_agent_api::AgentContinuationRef {
            opaque_id: "continuation-1".into()
        })]
    );

    // The journal keeps the original domain command, including its original cursor.
    let command = agent_protocol::AgentExecutionCommand::Run {
        params: Box::new(response.clone()),
    };
    assert!(store
        .admission(&scope, &identity, &command)
        .await
        .unwrap()
        .is_some());
    let mut replay = harness.start(response.clone()).await.unwrap();
    replay.admission.permit.consume().await.unwrap();
    while replay.events.recv().await.is_some() {}
    replay.settlement.recv().await.unwrap();
    assert_eq!(provider.continuations.lock().unwrap().len(), 1);
    let mut conflicting = response.clone();
    conflicting.input.created_at += 1;
    let error = match harness.start(conflicting).await {
        Ok(_) => panic!("changed response was accepted"),
        Err(error) => error,
    };
    assert!(error.is_code(ErrorCode::IdempotencyConflict));

    harness
        .interrupt(agent_protocol::AgentInterruptParams {
            agent_id: "agent-1".into(),
            identity: identity.clone(),
        })
        .await
        .unwrap();
    let current = store
        .execution_state(&scope)
        .await
        .unwrap()
        .unwrap()
        .history_cursor;
    harness.clear_session(scope, current).await.unwrap();
    let error = match harness.start(response).await {
        Ok(_) => panic!("stale response was accepted"),
        Err(error) => error,
    };
    assert!(error.is_code(ErrorCode::EventCursorExpired));
}

#[tokio::test]
async fn waiting_worker_rejects_wrong_action_and_typed_response_before_provider_execution() {
    let (_directory, store, harness, provider) = fixture(FakeMode::Waiting).await;
    let identity = AgentTurnRef::new("session-1", "turn-waiting");
    let execution = harness
        .start(run_params(identity.clone(), "message-waiting"))
        .await
        .unwrap();
    execution.admission.permit.consume().await.unwrap();
    drop(execution.events);
    execution.settlement.recv().await.unwrap();
    let before = store.checkpoint(&identity).await.unwrap().unwrap();
    let invalid_action =
        super::test_support::response_params(identity.clone(), "answer-1", "other-action");
    let error = match harness.start(invalid_action).await {
        Ok(_) => panic!("wrong action was accepted"),
        Err(error) => error,
    };
    assert!(error.is_code(ErrorCode::Conflict));
    let mut invalid_response =
        super::test_support::response_params(identity.clone(), "answer-2", "action-1");
    let talos_agent_api::AgentMessageBlockContent::UserContent(
        talos_agent_api::AgentUserContent::UserActionResponse { response, .. },
    ) = &mut invalid_response.input.blocks[0].content
    else {
        unreachable!()
    };
    *response = talos_types::UserActionResponse::SandboxApproval(
        talos_types::SandboxApprovalDecision::AllowOnce,
    );
    let error = match harness.start(invalid_response).await {
        Ok(_) => panic!("mismatched response was accepted"),
        Err(error) => error,
    };
    assert!(error.is_code(ErrorCode::InvalidRequest));
    assert_eq!(store.checkpoint(&identity).await.unwrap().unwrap(), before);
    assert_eq!(*provider.continuations.lock().unwrap(), vec![None]);
}

#[tokio::test]
async fn waiting_worker_requires_its_local_checkpoint_before_provider_execution() {
    let (directory, _store, harness, provider) = fixture(FakeMode::Waiting).await;
    let identity = AgentTurnRef::new("session-1", "turn-waiting");
    let execution = harness
        .start(run_params(identity.clone(), "message-waiting"))
        .await
        .unwrap();
    execution.admission.permit.consume().await.unwrap();
    drop(execution.events);
    execution.settlement.recv().await.unwrap();
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(directory.path().join("worker.sqlite")),
    )
    .await
    .unwrap();
    sqlx::query("DELETE FROM turn_checkpoints WHERE session_id = ? AND turn_id = ?")
        .bind(&identity.session_id)
        .bind(&identity.turn_id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let response = super::test_support::response_params(identity, "answer-1", "action-1");
    let error = match harness.start(response).await {
        Ok(_) => panic!("missing local checkpoint was accepted"),
        Err(error) => error,
    };
    assert!(error.is_code(ErrorCode::Conflict));
    assert_eq!(*provider.continuations.lock().unwrap(), vec![None]);
}

#[tokio::test]
async fn recreated_worker_resolves_private_waiting_state_without_exporting_it() {
    let (directory, store, harness, _) = fixture(FakeMode::Waiting).await;
    let identity = AgentTurnRef::new("session-1", "restart-waiting");
    let params = run_params(identity.clone(), "question");
    let scope = params.context.session.clone();
    let execution = harness.start(params).await.unwrap();
    execution.admission.permit.consume().await.unwrap();
    drop(execution.events);
    let settlement = execution.settlement.recv().await.unwrap();
    let agent_protocol::AgentRunSettlement::WaitingForInput { event: waiting, .. } = settlement
    else {
        panic!("expected waiting fact")
    };
    harness
        .shutdown(std::time::Duration::from_secs(2))
        .await
        .unwrap();
    drop(harness);
    drop(store);

    let store = Arc::new(
        WorkerExecutionStore::open(&directory.path().join("worker.sqlite"))
            .await
            .unwrap(),
    );
    let provider = FakeProvider::new(FakeMode::Waiting);
    let harness = LocalExecutionHarness::new(
        Arc::new(Harness::new(
            "agent-1".into(),
            provider.clone(),
            store.clone(),
        )),
        store.clone(),
    );
    let page = store
        .read_events(AgentSessionEventsParams {
            session: scope.clone(),
            after_cursor: None,
            limit: 256,
        })
        .await
        .unwrap();
    assert_eq!(page.events.last().unwrap().event, waiting);
    assert!(!serde_json::to_string(&page)
        .unwrap()
        .contains("continuation-1"));
    let params = super::test_support::response_params(identity.clone(), "answer", "action-1");
    assert!(!serde_json::to_string(&params)
        .unwrap()
        .contains("continuation-1"));
    let execution = harness.start(params).await.unwrap();
    execution.admission.permit.consume().await.unwrap();
    drop(execution.events);
    assert!(matches!(
        execution.settlement.recv().await.unwrap(),
        agent_protocol::AgentRunSettlement::WaitingForInput { .. }
    ));
    assert_eq!(
        *provider.continuations.lock().unwrap(),
        vec![Some(talos_agent_api::AgentContinuationRef {
            opaque_id: "continuation-1".into()
        })]
    );
    harness
        .interrupt(agent_protocol::AgentInterruptParams {
            agent_id: "agent-1".into(),
            identity,
        })
        .await
        .unwrap();
    harness
        .shutdown(std::time::Duration::from_secs(2))
        .await
        .unwrap();
}

#[tokio::test]
async fn stopped_worker_rejects_a_paused_permit_without_admitting_or_starting_provider() {
    let (_directory, store, harness, provider) = fixture(FakeMode::Release).await;
    let params = run_params(AgentTurnRef::new("session-1", "paused-stop"), "message-1");
    let scope = params.context.session.clone();
    let execution = harness.start(params).await.unwrap();
    harness
        .stop_execution(std::time::Duration::from_secs(1))
        .await
        .unwrap();
    assert!(execution
        .admission
        .permit
        .consume()
        .await
        .unwrap_err()
        .is_code(ErrorCode::Cancelled));
    assert!(!provider.started.load(std::sync::atomic::Ordering::Acquire));
    assert!(store.execution_state(&scope).await.unwrap().is_none());
    assert_eq!(
        provider
            .shutdown_count
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
    harness
        .shutdown(std::time::Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(
        provider
            .shutdown_count
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
}

#[tokio::test]
async fn stopping_execution_waits_for_the_local_terminal_journal_commit() {
    let (_directory, store, harness, provider) = fixture(FakeMode::Release).await;
    let params = run_params(AgentTurnRef::new("session-1", "running-stop"), "message-1");
    let scope = params.context.session.clone();
    let execution = harness.start(params).await.unwrap();
    execution.admission.permit.consume().await.unwrap();
    provider.wait_started().await;
    drop(execution.events);
    harness
        .stop_execution(std::time::Duration::from_secs(1))
        .await
        .unwrap();
    let page = store
        .read_events(AgentSessionEventsParams {
            session: scope,
            after_cursor: None,
            limit: 256,
        })
        .await
        .unwrap();
    assert!(page.active_turn.is_none());
    assert!(
        matches!(&page.events.last().unwrap().event.payload, AgentEventPayload::TurnFailed(failure) if failure.error.code == ErrorCode::Cancelled)
    );
    assert!(page.events.last().unwrap().settlement.is_some());
    assert_eq!(
        provider
            .shutdown_count
            .load(std::sync::atomic::Ordering::Acquire),
        0
    );
    harness
        .shutdown(std::time::Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(
        provider
            .shutdown_count
            .load(std::sync::atomic::Ordering::Acquire),
        1
    );
}
