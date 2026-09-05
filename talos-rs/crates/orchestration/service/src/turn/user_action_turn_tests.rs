use super::*;
use talos_agent_api::{AgentTurnRef, SubmitTurnInput, SubmitTurnInputRequest};

#[tokio::test]
async fn waiting_input_replays_the_committed_response_without_claiming_twice() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    let (service, _event_service, store, _message_queue) =
        test_service(agent_execution.clone()).await;
    create_test_session_record(store.clone()).await;
    insert_pending_dynamic_action(store.as_ref(), Some("tool-original")).await;
    let request = SubmitTurnInputRequest {
        identity: AgentTurnRef::new("s1", "turn-original"),
        input: SubmitTurnInput::UserActionResponse {
            action_id: "action-1".to_string(),
            response: UserActionResponse::DynamicToolResult(DynamicToolResult {
                output: "ok".to_string(),
                success: true,
            }),
        },
        client_request_id: Some("action-request-1".to_string()),
    };

    let first = service
        .submit_turn_input(test_session(), request.clone())
        .await
        .unwrap();
    let replay = service
        .submit_turn_input(test_session(), request)
        .await
        .unwrap();

    assert_eq!(replay, first);
    assert_eq!(agent_execution.stream_requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn dynamic_tool_response_resumes_the_original_canonical_turn() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    let (service, _event_service, store, _message_queue) =
        test_service(agent_execution.clone()).await;
    create_test_session_record(store.clone()).await;
    insert_pending_dynamic_action(store.as_ref(), Some("tool-original")).await;

    service
        .submit_user_action(
            test_session(),
            "turn-original",
            "action-1",
            UserActionResponse::DynamicToolResult(talos_types::DynamicToolResult {
                output: "ok".to_string(),
                success: true,
            }),
            None,
        )
        .await
        .expect("dynamic Tool response should resume the waiting Turn");
    wait_for_canonical_terminal(store.as_ref()).await;

    let request = agent_execution
        .stream_request
        .lock()
        .unwrap()
        .clone()
        .expect("resume request should be captured");
    assert_eq!(
        (
            request.context.turn_id,
            request.input.message_id,
            request.checkpoint.is_some(),
            request.continuation.is_some(),
        ),
        (
            "turn-original".to_string(),
            "msg_action_action-1".to_string(),
            true,
            true,
        )
    );
    let events = store
        .list_agent_turn_events(&AgentTurnRef::new("s1", "turn-original"), 64)
        .await
        .unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        CanonicalAgentEventPayload::MessageCompleted(message)
            if message.message_id == "msg_action_action-1"
    )));
    assert!(matches!(
        events.last().map(|event| &event.payload),
        Some(CanonicalAgentEventPayload::TurnCompleted(_))
    ));
}

#[tokio::test]
async fn sandbox_cancel_is_one_canonical_response_and_one_terminal_fact() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    *agent_execution.turn_errors_remaining.lock().unwrap() = 1;
    let coordinator = Arc::new(RecordingAgentCoordinator::default());
    let (service, _event_service, store, _message_queue) =
        test_service_with_coordinator(agent_execution, coordinator.clone()).await;
    create_test_session_record(store.clone()).await;
    insert_pending_sandbox_approval(store.as_ref()).await;
    let response = UserActionResponse::SandboxApproval(SandboxApprovalDecision::Cancel);

    let accepted = service
        .submit_user_action(
            test_session(),
            "turn-original",
            "action-1",
            response.clone(),
            None,
        )
        .await
        .expect("cancel response should be accepted once");
    assert_eq!(accepted, UserActionState::Resuming);
    wait_for_canonical_terminal(store.as_ref()).await;

    let replay = service
        .submit_user_action(test_session(), "turn-original", "action-1", response, None)
        .await
        .expect("same canonical response should be idempotent");
    assert_eq!(replay, UserActionState::Cancelled);
    assert!(coordinator.cancelled_turns.lock().unwrap().is_empty());

    let events = store
        .list_agent_turn_events(&AgentTurnRef::new("s1", "turn-original"), 64)
        .await
        .unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                &event.payload,
                CanonicalAgentEventPayload::MessageCompleted(message)
                    if message.message_id == "msg_action_action-1"
            ))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event.payload,
                CanonicalAgentEventPayload::TurnCompleted(_)
                    | CanonicalAgentEventPayload::TurnFailed(_)
            ))
            .count(),
        1
    );
}

#[tokio::test]
async fn stale_user_action_response_cannot_reopen_a_terminal_turn() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    let (service, _event_service, store, _message_queue) =
        test_service(agent_execution.clone()).await;
    insert_pending_dynamic_action(store.as_ref(), Some("tool-original")).await;
    settle_pending_turn(store.as_ref()).await;

    let error = service
        .submit_user_action(
            test_session(),
            "turn-original",
            "action-1",
            UserActionResponse::DynamicToolResult(DynamicToolResult {
                output: "ok".to_string(),
                success: true,
            }),
            None,
        )
        .await
        .expect_err("terminal Turn must reject a new action response");

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    assert!(agent_execution.stream_request.lock().unwrap().is_none());
}

async fn settle_pending_turn(store: &DatabaseSessionStore) {
    let identity = AgentTurnRef::new("s1", "turn-original");
    let active = store
        .get_agent_active_turn("s1")
        .await
        .unwrap()
        .expect("waiting canonical Turn should exist");
    let events = store.list_agent_turn_events(&identity, 64).await.unwrap();
    store
        .commit_terminal_settlement(talos_store::session::CommitTerminalSettlementParams {
            event: CanonicalAgentEvent {
                identity,
                sequence: events.last().unwrap().sequence.saturating_add(1),
                occurred_at: 10,
                payload: CanonicalAgentEventPayload::TurnFailed(AgentTurnFailed {
                    error: AgentErrorInfo {
                        code: talos_error::ErrorCode::Cancelled,
                        message: "settled before response".to_string(),
                        retryable: false,
                    },
                }),
            },
            expected_event_seq: active.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .expect("terminal canonical fact should commit");
}
