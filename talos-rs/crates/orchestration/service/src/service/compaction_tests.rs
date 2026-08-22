use super::*;

#[tokio::test]
async fn explicit_compaction_records_maintenance_usage_and_preserves_measured_context() {
    let compaction_usage = TokenUsage {
        input_tokens: 3,
        output_tokens: 2,
        total_tokens: 5,
        ..TokenUsage::default()
    };
    let runtime = Arc::new(TestSessionRuntime {
        cancel_mode: TestCancelMode::Cancelled,
        cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
        compaction_usage,
    });
    let (service, store) = test_application_with_runtime(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        runtime,
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    let session_id = "session-compact-usage";

    service
        .create_session(
            &actor,
            create_session_params(session_id, request_context(), None),
        )
        .await
        .unwrap();

    let measured_context = talos_types::ContextUsage::new(20, 100, 80, 1).unwrap();
    let mut record = store.get_session_record(session_id).await.unwrap().unwrap();
    record.usage = talos_types::SessionUsage {
        tokens: TokenUsage {
            input_tokens: 10,
            output_tokens: 4,
            total_tokens: 14,
            ..TokenUsage::default()
        },
        context: Some(measured_context.clone()),
    };
    store.update_session_record(&record).await.unwrap();

    let compacted = service.compact_session(&actor, session_id).await.unwrap();
    let reloaded = service.get_session(&actor, session_id).await.unwrap();
    let stored = store.get_session_record(session_id).await.unwrap().unwrap();
    let expected = talos_types::SessionUsage {
        tokens: TokenUsage {
            input_tokens: 13,
            output_tokens: 6,
            total_tokens: 19,
            ..TokenUsage::default()
        },
        context: Some(measured_context),
    };

    assert_eq!(
        [compacted.state.usage, reloaded.state.usage, stored.usage],
        [expected.clone(), expected.clone(), expected]
    );
}

#[tokio::test]
async fn explicit_compaction_persists_the_existing_timeline_events() {
    let (service, _store) = test_application(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-compact-events", request_context(), None),
        )
        .await
        .expect("session should be created");

    let session = service
        .compact_session(&actor, "session-compact-events")
        .await
        .expect("compaction should complete");
    assert_eq!(session.state.message_count, 0);

    let recovery = service
        .events
        .recover_timeline("session-compact-events", None, 16)
        .await
        .expect("compaction timeline should be recoverable");
    let events = recovery.replay.events;
    assert_eq!(events.len(), 5);
    assert_eq!(
        events
            .iter()
            .map(|event| event.event.event_type)
            .collect::<Vec<_>>(),
        vec![
            talos_types::talos::AgentEventType::TurnStarted,
            talos_types::talos::AgentEventType::ItemStarted,
            talos_types::talos::AgentEventType::ItemDelta,
            talos_types::talos::AgentEventType::ItemCompleted,
            talos_types::talos::AgentEventType::TurnCompleted,
        ]
    );
    assert!(events.iter().all(|event| {
        event.session_id == "session-compact-events"
            && event.turn_id.starts_with("turn_")
            && event.message_id.starts_with("msg_")
            && event.turn_id.strip_prefix("turn_") == event.message_id.strip_prefix("msg_")
    }));
    let request_id = match &events[0].event.payload {
        AgentEventPayload::TurnStarted(payload) => payload.request_id.as_str(),
        _ => unreachable!("first compaction event should start the maintenance turn"),
    };
    assert_eq!(
        request_id.strip_prefix("req_"),
        events[0].turn_id.strip_prefix("turn_")
    );
    assert_eq!(
        recovery.active_turn.status,
        talos_types::TimelineActiveTurnStatus::Completed
    );
}

#[tokio::test]
async fn explicit_compaction_records_terminal_after_an_intermediate_event_failure() {
    let runtime = Arc::new(TestSessionRuntime {
        cancel_mode: TestCancelMode::Cancelled,
        cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
        compaction_usage: TokenUsage::default(),
    });
    let (service, store) = test_application_with_runtime_and_event_failure(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        runtime,
        configured_test_config(),
        Some(2),
    )
    .await;
    let actor = Actor::regular("member-user");
    let session_id = "session-compact-event-failure";
    service
        .create_session(
            &actor,
            create_session_params(session_id, request_context(), None),
        )
        .await
        .expect("session should be created");

    let error = service
        .compact_session(&actor, session_id)
        .await
        .expect_err("the first persistence error should be returned");

    assert!(error.is_internal());
    assert_eq!(
        error.diagnostic_message(),
        "injected compaction event persistence failure"
    );
    assert_eq!(
        store
            .get_active_turn(session_id)
            .await
            .expect("active turn read")
            .expect("compaction active turn")
            .status,
        ActiveTurnStatus::Completed
    );
    let events = store
        .list_events_after(session_id, None, 16)
        .await
        .expect("persisted compaction events");
    assert_eq!(
        events.last().map(|event| event.event_type),
        Some(talos_types::talos::AgentEventType::TurnCompleted)
    );
}
