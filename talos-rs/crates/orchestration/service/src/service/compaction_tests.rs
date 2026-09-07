use super::*;

#[tokio::test]
async fn explicit_compaction_returns_original_failure_after_durable_settlement() {
    let original = TalosError::from_source(
        talos_error::ErrorCode::ProviderUnavailable,
        "Compaction provider disconnected",
        std::io::Error::other("original transport failure"),
    );
    let coordinator = Arc::new(TestAgentCoordinator {
        cancel_mode: TestCancelMode::Cancelled,
        cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
        compaction_usage: TokenUsage::default(),
        compaction_error: Some(original.clone()),
    });
    let (service, _) = test_application_with_coordinator(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        coordinator,
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("compact-failed", request_source(), None),
        )
        .await
        .expect("create session");
    let error = service
        .compact_session(&actor, "compact-failed")
        .await
        .expect_err("return failure");
    assert_eq!(error.error_code(), original.error_code());
    assert!(std::ptr::eq(error.backtrace(), original.backtrace()));
    assert!(std::error::Error::source(&error)
        .unwrap()
        .is::<std::io::Error>());
    let recovery = service
        .events
        .recover_timeline("compact-failed", None, 16)
        .await
        .unwrap();
    assert!(matches!(
        recovery
            .replay
            .events
            .last()
            .unwrap()
            .event
            .agent_event()
            .unwrap()
            .payload,
        CanonicalAgentEventPayload::TurnFailed(_)
    ));
    assert_eq!(recovery.active_turn, None);
}

#[tokio::test]
async fn explicit_compaction_records_maintenance_usage_and_preserves_measured_context() {
    let compaction_usage = TokenUsage {
        input_tokens: 3,
        output_tokens: 2,
        total_tokens: 5,
        ..TokenUsage::default()
    };
    let coordinator = Arc::new(TestAgentCoordinator {
        cancel_mode: TestCancelMode::Cancelled,
        cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
        compaction_usage: compaction_usage.clone(),
        compaction_error: None,
    });
    let (service, store) = test_application_with_coordinator(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        coordinator,
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    let session_id = "session-compact-usage";

    service
        .create_session(
            &actor,
            create_session_params(session_id, request_source(), None),
        )
        .await
        .unwrap();

    let measured_context = talos_types::ContextUsage::new(20, 100, 80, 1).unwrap();
    store
        .accumulate_session_usage(AccumulateSessionUsageParams {
            session_id: session_id.to_string(),
            tokens: TokenUsage {
                input_tokens: 10,
                output_tokens: 4,
                total_tokens: 14,
                ..TokenUsage::default()
            },
            last_activity: 1,
        })
        .await
        .unwrap();
    store
        .update_context_usage(session_id, &measured_context)
        .await
        .unwrap();

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
        [compacted.usage, reloaded.usage, stored.usage],
        [expected.clone(), expected.clone(), expected]
    );
    let profile = service.profile.read_profile(&actor).await.unwrap();
    assert_eq!(profile.usage, compaction_usage);
    assert_eq!(profile.usage_session_count, 0);
    assert_eq!(profile.model_usage[0].turn_count, 0);
}

#[tokio::test]
async fn explicit_compaction_persists_one_canonical_lifecycle() {
    let (service, _store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-compact-events", request_source(), None),
        )
        .await
        .expect("session should be created");

    let session = service
        .compact_session(&actor, "session-compact-events")
        .await
        .expect("compaction should complete");
    assert_eq!(session.message_count, 0);

    let recovery = service
        .events
        .recover_timeline("session-compact-events", None, 16)
        .await
        .expect("compaction timeline should be recoverable");
    let events = recovery.replay.events;
    assert_eq!(events.len(), 7);
    assert!(matches!(
        events[0].event.agent_event().unwrap().payload,
        CanonicalAgentEventPayload::TurnStarted(_)
    ));
    assert!(matches!(
        events[1].event.agent_event().unwrap().payload,
        CanonicalAgentEventPayload::MessageStarted(_)
    ));
    assert!(matches!(
        events[2].event.agent_event().unwrap().payload,
        CanonicalAgentEventPayload::MessageBlockStarted(_)
    ));
    assert!(matches!(
        events[3].event.agent_event().unwrap().payload,
        CanonicalAgentEventPayload::MessageBlockDelta(_)
    ));
    assert!(matches!(
        events[4].event.agent_event().unwrap().payload,
        CanonicalAgentEventPayload::MessageBlockCompleted(_)
    ));
    assert!(matches!(
        events[5].event.agent_event().unwrap().payload,
        CanonicalAgentEventPayload::MessageCompleted(_)
    ));
    assert!(matches!(
        events[6].event.agent_event().unwrap().payload,
        CanonicalAgentEventPayload::TurnCompleted(_)
    ));
    let identity = &events[0].event.agent_event().unwrap().identity;
    assert_eq!(identity.session_id, "session-compact-events");
    assert!(events
        .iter()
        .all(|event| event.event.agent_event().unwrap().identity == *identity));
    assert_eq!(recovery.active_turn, None);
}
