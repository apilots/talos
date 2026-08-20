use super::*;

#[tokio::test]
async fn interrupt_requires_the_current_turn_and_pauses_until_explicit_queue_start() {
    let cancelled_turn_ids = Arc::new(Mutex::new(Vec::new()));
    let (service, store) = test_application_with_runtime(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Accepted,
        }),
        Arc::new(TestSessionRuntime {
            cancel_mode: TestCancelMode::Ok,
            cancelled_turn_ids: cancelled_turn_ids.clone(),
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-interrupt-queue", request_context(), None),
        )
        .await
        .expect("session should be created");
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "session-interrupt-queue".to_string(),
            turn_id: "turn-current".to_string(),
            message_id: Some("msg-current".to_string()),
            status: ActiveTurnStatus::Running,
            last_event_id: None,
            current_action_id: None,
            updated_at: 1,
        })
        .await
        .expect("active turn should be inserted");

    let stale_error = service
        .interrupt_turn(
            &actor,
            talos_types::InterruptTurnRequest {
                session_id: "session-interrupt-queue".to_string(),
                turn_id: "turn-stale".to_string(),
            },
        )
        .await
        .expect_err("stale interrupt must not cancel the current turn");
    assert!(stale_error.is_code(talos_error::ErrorCode::ResourceNotFound));
    assert!(cancelled_turn_ids.lock().unwrap().is_empty());

    service
        .interrupt_turn(
            &actor,
            talos_types::InterruptTurnRequest {
                session_id: "session-interrupt-queue".to_string(),
                turn_id: "turn-current".to_string(),
            },
        )
        .await
        .expect("current turn interrupt should be accepted");
    assert_eq!(
        *cancelled_turn_ids.lock().unwrap(),
        vec!["turn-current".to_string()]
    );
    let cancelling_turn = store
        .get_active_turn("session-interrupt-queue")
        .await
        .expect("active turn should load after interrupt")
        .expect("interrupted turn should remain available for recovery");
    assert_eq!(cancelling_turn.status, ActiveTurnStatus::Cancelling);

    service
        .event_log
        .record_talos_event(AgentEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: Some(2),
            event_type: talos_types::talos::AgentEventType::TurnCompleted,
            turn_id: "turn-current".to_string(),
            message_id: "msg-current".to_string(),
            session_id: "session-interrupt-queue".to_string(),
            sequence: 1,
            payload: AgentEventPayload::TurnCompleted(TalosTurnCompletedEvent {
                request_id: "request-current".to_string(),
                turn_id: "turn-current".to_string(),
                completed_at_ms: 2,
                stop_reason: StopReason::Cancelled,
                usage: TokenUsage::default(),
                metrics: None,
                artifact_seq: None,
            }),
        })
        .await
        .expect("cancelled turn should become terminal");
    let queue = service
        .message_queue
        .add(
            "session-interrupt-queue",
            vec![TalosInputItem::Text {
                text: "continue explicitly".to_string(),
            }],
        )
        .unwrap();
    assert!(service
        .turns
        .try_dispatch_next("session-interrupt-queue")
        .await
        .unwrap()
        .is_none());

    let session = service
        .access
        .get_session(&actor, "session-interrupt-queue")
        .await
        .expect("session should be readable");
    service
        .turns
        .start_queued_message(session, Some(&queue.messages[0].id))
        .await
        .expect("explicit start should resume queue execution");

    assert!(service
        .message_queue
        .list("session-interrupt-queue")
        .unwrap()
        .messages
        .is_empty());
}

#[tokio::test]
async fn interrupt_restores_active_turn_when_runtime_rejects_cancellation() {
    let (service, store) = test_application_with_runtime(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        Arc::new(TestSessionRuntime {
            cancel_mode: TestCancelMode::Rejected,
            cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-interrupt-rejected", request_context(), None),
        )
        .await
        .expect("session should be created");
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "session-interrupt-rejected".to_string(),
            turn_id: "turn-existing".to_string(),
            message_id: Some("msg-existing".to_string()),
            status: ActiveTurnStatus::Running,
            last_event_id: None,
            current_action_id: None,
            updated_at: 1,
        })
        .await
        .expect("active turn should be inserted");

    let error = service
        .interrupt_turn(
            &actor,
            talos_types::InterruptTurnRequest {
                session_id: "session-interrupt-rejected".to_string(),
                turn_id: "turn-existing".to_string(),
            },
        )
        .await
        .expect_err("runtime rejection should be returned");

    assert!(error
        .diagnostic_message()
        .contains("runtime rejected cancellation"));
    let active_turn = store
        .get_active_turn("session-interrupt-rejected")
        .await
        .expect("active turn should load")
        .expect("active turn should remain available");
    assert_eq!(active_turn.status, ActiveTurnStatus::Running);
}
