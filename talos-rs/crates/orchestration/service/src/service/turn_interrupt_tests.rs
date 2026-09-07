use super::*;

#[tokio::test]
async fn interrupt_turn_without_matching_active_turn_returns_not_found() {
    let (service, _store) = test_application_with_coordinator(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        Arc::new(TestAgentCoordinator {
            cancel_mode: TestCancelMode::MissingLiveTurn,
            cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
            compaction_usage: TokenUsage::default(),
            compaction_error: None,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-interrupt-idempotent", request_source(), None),
        )
        .await
        .expect("session should be created");

    let error = service
        .interrupt_turn(
            &actor,
            talos_agent_api::InterruptTurnRequest {
                identity: talos_types::AgentTurnRef::new(
                    "session-interrupt-idempotent",
                    "turn-missing",
                ),
            },
        )
        .await
        .expect_err("interrupt without matching active turn should fail");

    assert!(error.is_code(talos_error::ErrorCode::ResourceNotFound));
}

#[tokio::test]
async fn interrupt_turn_unknown_session_returns_not_found() {
    let (service, _store) = test_application_with_coordinator(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        Arc::new(TestAgentCoordinator {
            cancel_mode: TestCancelMode::MissingLiveTurn,
            cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
            compaction_usage: TokenUsage::default(),
            compaction_error: None,
        }),
        configured_test_config(),
    )
    .await;

    let error = service
        .interrupt_turn(
            &Actor::regular("member-user"),
            talos_agent_api::InterruptTurnRequest {
                identity: talos_types::AgentTurnRef::new("missing-session", "turn-missing"),
            },
        )
        .await
        .expect_err("unknown session should be rejected");

    assert!(error.is_code(talos_error::ErrorCode::ResourceNotFound));
}

#[tokio::test]
async fn interrupt_turn_finalizes_orphaned_canonical_turn() {
    let cancelled_turn_ids = Arc::new(Mutex::new(Vec::new()));
    let (service, store) = test_application_with_coordinator(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        Arc::new(TestAgentCoordinator {
            cancel_mode: TestCancelMode::MissingLiveTurn,
            cancelled_turn_ids: cancelled_turn_ids.clone(),
            compaction_usage: TokenUsage::default(),
            compaction_error: None,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-interrupt-live-missing", request_source(), None),
        )
        .await
        .expect("session should be created");
    let identity =
        talos_agent_api::AgentTurnRef::new("session-interrupt-live-missing", "turn-existing");
    let message = crate::turn::completed_agent_user_input(
        &identity.turn_id,
        "msg-existing",
        crate::test_support::test_user_input("running"),
        1,
    )
    .unwrap();
    crate::test_support::commit_test_running_turn(store.as_ref(), identity.clone(), message).await;

    let response = service
        .interrupt_turn(
            &actor,
            talos_agent_api::InterruptTurnRequest {
                identity: talos_types::AgentTurnRef::new(
                    "session-interrupt-live-missing",
                    "turn-existing",
                ),
            },
        )
        .await
        .expect("orphaned active turn should be finalized");

    assert_eq!(
        response,
        talos_agent_api::InterruptTurnResponse {
            identity: talos_types::AgentTurnRef::new(
                "session-interrupt-live-missing",
                "turn-existing",
            ),
            outcome: talos_agent_api::AgentInterruptOutcome::Cancelled,
        }
    );
    assert_eq!(
        *cancelled_turn_ids.lock().unwrap(),
        vec!["turn-existing".to_string()]
    );
    assert!(store
        .get_agent_active_turn("session-interrupt-live-missing")
        .await
        .expect("active Turn lookup should succeed")
        .is_none());
    let events = store
        .list_agent_turn_events(&identity, 32)
        .await
        .expect("turn events should load");
    assert!(matches!(
        events.last().map(|event| &event.payload),
        Some(talos_agent_api::AgentEventPayload::TurnFailed(failed))
            if failed.error.code == talos_error::ErrorCode::Cancelled
    ));
}
