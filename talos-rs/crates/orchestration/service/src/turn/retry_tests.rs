use super::*;

#[tokio::test]
async fn retry_accepts_the_exact_initial_message_of_a_failed_canonical_turn() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    let (service, _event_service, store, _message_queue) = test_service(agent_execution).await;
    create_test_session_record(store.clone()).await;
    commit_terminal_source(
        store.as_ref(),
        "message-failed",
        CanonicalAgentEventPayload::TurnFailed(AgentTurnFailed {
            error: AgentErrorInfo {
                code: talos_error::ErrorCode::InternalError,
                message: "failed".to_string(),
                retryable: true,
            },
        }),
    )
    .await;

    let response = service
        .submit_retry(
            test_session(),
            TurnRetryParams {
                input: test_user_input("retry"),
                client_message_id: "message-retry".to_string(),
                retry_of_message_id: "message-failed".to_string(),
            },
        )
        .await
        .expect("failed canonical Turn should be retryable");

    assert_eq!(response.status, talos_types::TurnCommandStatus::Accepted);
}

#[tokio::test]
async fn retry_rejects_a_completed_canonical_turn() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    let (service, _event_service, store, _message_queue) = test_service(agent_execution).await;
    create_test_session_record(store.clone()).await;
    commit_terminal_source(
        store.as_ref(),
        "message-completed",
        CanonicalAgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    )
    .await;

    let error = service
        .submit_retry(
            test_session(),
            TurnRetryParams {
                input: test_user_input("retry"),
                client_message_id: "message-retry".to_string(),
                retry_of_message_id: "message-completed".to_string(),
            },
        )
        .await
        .expect_err("completed canonical Turn must not be retried");

    assert!(error.is_code(talos_error::ErrorCode::RetrySourceCompleted));
}

async fn commit_terminal_source(
    store: &DatabaseSessionStore,
    message_id: &str,
    payload: CanonicalAgentEventPayload,
) {
    let identity = talos_agent_api::AgentTurnRef::new("s1", "turn-original");
    let message = completed_agent_user_input(
        &identity.turn_id,
        message_id,
        test_user_input("original"),
        1,
    )
    .expect("canonical User message should be valid");
    let admission = commit_test_running_turn(store, identity.clone(), message).await;
    store
        .commit_terminal_settlement(talos_store::session::CommitTerminalSettlementParams {
            event: CanonicalAgentEvent {
                identity,
                sequence: admission
                    .events
                    .last()
                    .expect("canonical admission event")
                    .sequence
                    .saturating_add(1),
                occurred_at: 2,
                payload,
            },
            expected_event_seq: admission.active_turn.last_event_seq,
            completed_turn_metrics: None,
        })
        .await
        .expect("canonical terminal source should commit");
}
