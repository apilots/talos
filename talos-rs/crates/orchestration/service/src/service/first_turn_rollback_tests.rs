use super::*;

#[tokio::test]
async fn first_turn_start_timeout_does_not_leave_session_record() {
    let (service, store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartTimeout,
        }),
        configured_test_config(),
    )
    .await;

    let result = service
        .start_turn(
            &Actor::regular("member-user"),
            request_source(),
            start_turn_request("session-timeout", "hello"),
            Some("timeout-key".to_string()),
        )
        .await;

    assert!(result.is_err());
    assert_session_creation_rolled_back(&service, &store, "session-timeout", Some("timeout-key"))
        .await;
}

#[tokio::test]
async fn first_turn_start_failure_does_not_leave_session_record() {
    let (service, store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        configured_test_config(),
    )
    .await;

    let result = service
        .start_turn(
            &Actor::regular("member-user"),
            request_source(),
            start_turn_request("session-closed", "hello"),
            Some("closed-key".to_string()),
        )
        .await;

    assert!(result.is_err());
    assert_session_creation_rolled_back(&service, &store, "session-closed", Some("closed-key"))
        .await;
}

#[tokio::test]
async fn first_turn_idempotency_conflict_does_not_leave_session_record() {
    let (service, store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        configured_test_config(),
    )
    .await;
    let reservation = store
        .reserve_command(turn_submission_reservation(
            "session-idempotency-conflict",
            "conflict-key",
            "different-request-hash",
        ))
        .await
        .expect("idempotency reserve should succeed");
    assert!(matches!(reservation, IdempotencyReservation::Reserved(_)));

    let result = service
        .start_turn(
            &Actor::regular("member-user"),
            request_source(),
            start_turn_request("session-idempotency-conflict", "hello"),
            Some("conflict-key".to_string()),
        )
        .await;

    assert!(result
        .as_ref()
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::IdempotencyConflict)));
    assert_session_creation_rolled_back(&service, &store, "session-idempotency-conflict", None)
        .await;
}

#[tokio::test]
async fn first_turn_active_turn_conflict_does_not_leave_session_record() {
    let (service, store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config(),
    )
    .await;
    store
        .commit_maintenance_admission(CanonicalAgentEvent {
            identity: talos_agent_api::AgentTurnRef::new(
                "session-active-conflict",
                "turn-existing",
            ),
            sequence: 1,
            occurred_at: 1,
            payload: CanonicalAgentEventPayload::TurnStarted(AgentTurnStarted {}),
        })
        .await
        .expect("active Turn admission should succeed");

    let result = service
        .start_turn(
            &Actor::regular("member-user"),
            request_source(),
            start_turn_request("session-active-conflict", "hello"),
            None,
        )
        .await;

    assert!(result.is_err());
    assert_session_creation_rolled_back(&service, &store, "session-active-conflict", None).await;
}
