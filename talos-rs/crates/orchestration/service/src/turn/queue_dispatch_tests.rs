use super::*;
use talos_agent_api::AgentTurnRef;

fn log_dispatch(path: &std::path::Path) -> tracing::Dispatch {
    tracing::Dispatch::new(
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(Mutex::new(std::fs::File::create(path).unwrap()))
            .finish(),
    )
}

#[tokio::test]
async fn profile_write_failure_is_reported_once_without_stopping_queued_turns() {
    let logs = tempfile::tempdir().unwrap();
    let log_path = logs.path().join("profile.log");
    let dispatch = log_dispatch(&log_path);
    let _subscriber = tracing::dispatcher::set_default(&dispatch);
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    let execution = Arc::new(RecordingAgentExecution::default());
    let (service, timeline, store, queue) = test_service_with_storage(
        execution.clone(),
        Arc::new(RecordingAgentCoordinator::default()),
        storage.clone(),
    )
    .await;
    create_test_session_record(store).await;
    sqlx::query("CREATE TRIGGER reject_profile BEFORE INSERT ON profile_turn_usage WHEN (SELECT COUNT(*) FROM agent_events WHERE json_extract(payload_json, '$.type') = 'turnCompleted') = 1 BEGIN SELECT RAISE(FAIL, 'profile-write-sentinel'); END")
        .execute(storage.pool()).await.unwrap();
    queue.add("s1", test_user_input("first")).unwrap();
    queue.add("s1", test_user_input("next")).unwrap();
    let mut first_client = timeline.subscribe_live_events();
    let mut second_client = timeline.subscribe_live_events();
    let first = service.try_dispatch_next("s1").await.unwrap().unwrap();
    wait_for_stream_requests(execution.as_ref(), 2).await;
    let replay = timeline.list_events_after("s1", None, 100).await.unwrap();
    let notice = replay
        .iter()
        .find(|event| {
            matches!(
                event.event,
                talos_agent_api::TimelineEventPayload::SystemNotice(_)
            )
        })
        .unwrap();
    for client in [&mut first_client, &mut second_client] {
        let mut delivered = None;
        while let Ok(event) = client.try_recv() {
            if event.event_id == notice.event_id {
                delivered = Some(event);
            }
        }
        assert_eq!(delivered.as_ref(), Some(notice));
    }
    let history = crate::SessionHistoryService::new(timeline.clone())
        .read("s1", None, None)
        .await
        .unwrap();
    let turn = history
        .data
        .iter()
        .find(|turn| turn.identity.turn_id == first.turn_id)
        .unwrap();
    assert!(matches!(
        turn.result,
        talos_agent_api::AgentRunResult::Completed(_)
    ));
    assert_eq!(turn.notices, vec![notice.clone()]);
    let talos_agent_api::TimelineEventPayload::SystemNotice(data) = &notice.event else {
        unreachable!()
    };
    assert!(
        matches!(&data.notice.content, talos_agent_api::SystemNoticeContent::Error(error) if error.code == talos_error::ErrorCode::InternalError)
    );
    let content = std::fs::read_to_string(log_path).unwrap();
    assert_eq!(
        content.matches("application failure").count(),
        1,
        "{content}"
    );
    assert!(
        content.contains("profile-write-sentinel") && content.contains("backtrace_status=Captured"),
        "{content}"
    );
}

#[tokio::test]
async fn profile_discard_failure_preserves_the_error_and_dispatches_queued_work() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    let execution = Arc::new(RecordingAgentExecution::default());
    let (service, _, store, queue) = test_service_with_storage(
        execution.clone(),
        Arc::new(RecordingAgentCoordinator::default()),
        storage.clone(),
    )
    .await;
    create_test_session_record(store.clone()).await;
    let identity = AgentTurnRef::new("s1", "failed-turn");
    let input = completed_agent_user_input("failed-turn", "message-1", test_user_input("first"), 1)
        .unwrap();
    commit_test_running_turn(store.as_ref(), identity, input).await;
    DatabaseProfileStore::new(storage.pool().clone())
        .record_turn_usage(talos_store::profile::TurnUsageUpdate {
            user_id: "u1".to_string(),
            session_id: "s1".to_string(),
            turn_id: "failed-turn".to_string(),
            request_id: "request-1".to_string(),
            model_ref: ModelRef {
                provider: "mock".to_string(),
                id: "model-1".to_string(),
            },
            usage: talos_types::TokenUsage::default(),
            usage_available: false,
            stop_reason: StopReason::WaitingUserAction,
            timezone: "UTC".to_string(),
            completed_at_ms: 1000,
            usage_date: "2026-09-06".to_string(),
        })
        .await
        .unwrap();
    sqlx::query("CREATE TRIGGER reject_profile_discard BEFORE DELETE ON profile_turn_usage BEGIN SELECT RAISE(FAIL, 'profile-discard-sentinel'); END")
        .execute(storage.pool()).await.unwrap();
    queue.add("s1", test_user_input("next")).unwrap();
    let active = store.get_agent_active_turn("s1").await.unwrap().unwrap();
    let error = service
        .recover_active_turn(test_session(), active)
        .await
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("profile-discard-sentinel"),
        "{error:?}"
    );
    wait_for_stream_requests(execution.as_ref(), 1).await;
}

#[tokio::test]
async fn committed_failure_recovery_releases_execution_and_dispatches_the_queue() {
    for rejected_stage in ["panic", "permit", "metricsUpdated", "turnCompleted"] {
        let logs = tempfile::tempdir().unwrap();
        let log_path = logs.path().join("recovery.log");
        let dispatch = log_dispatch(&log_path);
        let subscriber = tracing::dispatcher::set_default(&dispatch);
        let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
        let execution = Arc::new(RecordingAgentExecution::default());
        let coordinator = Arc::new(RecordingAgentCoordinator::default());
        if rejected_stage == "turnCompleted" {
            *coordinator.interrupt_error.lock().unwrap() = Some(TalosError::new(
                talos_error::ErrorCode::RunCleanupUnconfirmed,
                "The remote Client no longer owns a live invocation",
            ));
        }
        let (service, _, store, queue) =
            test_service_with_storage(execution.clone(), coordinator, storage.clone()).await;
        create_test_session_record(store.clone()).await;
        if rejected_stage == "metricsUpdated" {
            *execution.progress_event.lock().unwrap() = Some(
                CanonicalAgentEventPayload::MetricsUpdated(AgentRunMetrics::default()),
            );
        }
        if rejected_stage == "panic" {
            execution.panic_start_permit.store(true, Ordering::SeqCst);
        } else if rejected_stage == "permit" {
            *execution.start_permit_error.lock().unwrap() = Some(talos_error::TalosError::new(
                talos_error::ErrorCode::InternalError,
                "injected start permit rejection",
            ));
        } else {
            sqlx::query(&format!(
                r#"
        CREATE TRIGGER reject_first_completion BEFORE INSERT ON agent_events
        WHEN json_extract(NEW.payload_json, '$.type') = '{rejected_stage}'
          AND NOT EXISTS (
            SELECT 1 FROM agent_events WHERE session_id = NEW.session_id
              AND json_extract(payload_json, '$.type') = 'turnFailed'
          )
        BEGIN SELECT RAISE(FAIL, 'injected terminal persistence failure'); END
    "#
            ))
            .execute(storage.pool())
            .await
            .unwrap();
        }
        queue.add("s1", test_user_input("first")).unwrap();
        queue.add("s1", test_user_input("next")).unwrap();
        let first = service.try_dispatch_next("s1").await.unwrap().unwrap();
        wait_for_stream_requests(execution.as_ref(), 2).await;
        let events = store
            .list_agent_turn_events(&AgentTurnRef::new("s1", first.turn_id), 64)
            .await
            .unwrap();
        let terminal = events
            .iter()
            .filter(|event| {
                matches!(
                    event.payload,
                    CanonicalAgentEventPayload::TurnCompleted(_)
                        | CanonicalAgentEventPayload::TurnFailed(_)
                )
            })
            .collect::<Vec<_>>();
        assert!(
            matches!(terminal.as_slice(), [event] if matches!(&event.payload,
                CanonicalAgentEventPayload::TurnFailed(failure) if failure.error.code == talos_error::ErrorCode::InternalError
            ))
        );
        assert!(events
            .windows(2)
            .all(|pair| pair[1].sequence == pair[0].sequence + 1));
        assert_eq!(execution.stream_requests.lock().unwrap().len(), 2);
        drop(subscriber);
        drop(dispatch);
        let content = std::fs::read_to_string(log_path).unwrap();
        assert!(
            content.contains("injected"),
            "original diagnostics must reach the log"
        );
        assert_eq!(
            content.matches("application failure").count(),
            1,
            "{content}"
        );
    }
}

#[tokio::test]
async fn queued_messages_auto_dispatch_in_fifo_order_after_fast_completion() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    let (service, _event_service, store, message_queue) =
        test_service(agent_execution.clone()).await;
    create_test_session_record(store).await;
    message_queue.add("s1", test_user_input("first")).unwrap();
    message_queue.add("s1", test_user_input("second")).unwrap();

    service
        .try_dispatch_next("s1")
        .await
        .expect("first queued message should start")
        .expect("queue should contain a message");
    wait_for_stream_requests(agent_execution.as_ref(), 2).await;

    let request_texts = agent_execution
        .stream_requests
        .lock()
        .unwrap()
        .iter()
        .map(|request| match &request.input.blocks[0].content {
            talos_agent_api::AgentMessageBlockContent::UserContent(
                talos_agent_api::AgentUserContent::Text { text },
            ) => text.clone(),
            content => panic!("unexpected queued input: {content:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        request_texts,
        vec!["first".to_string(), "second".to_string()]
    );
    for _ in 0..20 {
        if message_queue.list("s1").unwrap().messages.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(message_queue.list("s1").unwrap().messages.is_empty());
}

#[tokio::test]
async fn queued_messages_continue_after_a_failed_turn() {
    let logs = tempfile::tempdir().unwrap();
    let log_path = logs.path().join("turn.log");
    let dispatch = log_dispatch(&log_path);
    let subscriber = tracing::dispatcher::set_default(&dispatch);
    let agent_execution = Arc::new(RecordingAgentExecution {
        turn_errors_remaining: Mutex::new(1),
        ..Default::default()
    });
    let (service, _event_service, store, message_queue) =
        test_service(agent_execution.clone()).await;
    create_test_session_record(store).await;
    message_queue.add("s1", test_user_input("fails")).unwrap();
    message_queue
        .add("s1", test_user_input("continues"))
        .unwrap();

    service
        .try_dispatch_next("s1")
        .await
        .expect("failed queued turn should still be accepted");
    wait_for_stream_requests(agent_execution.as_ref(), 2).await;
    for _ in 0..20 {
        if message_queue.list("s1").unwrap().messages.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(message_queue.list("s1").unwrap().messages.is_empty());
    drop(subscriber);
    drop(dispatch);
    let content = std::fs::read_to_string(log_path).unwrap();
    assert_eq!(
        content.matches("application failure").count(),
        1,
        "{content}"
    );
}

#[tokio::test]
async fn automatic_dispatch_contention_yields_but_manual_start_stays_busy() {
    let execution = Arc::new(RecordingAgentExecution::default());
    execution.hold_admission.store(true, Ordering::SeqCst);
    let (service, timeline, store, queue) = test_service(execution.clone()).await;
    create_test_session_record(store.clone()).await;
    let queued = queue
        .add("s1", test_user_input("run exactly once"))
        .unwrap();
    let message_id = queued.messages[0].id.clone();
    let winner = tokio::spawn({
        let service = service.clone();
        async move { service.try_dispatch_next("s1").await }
    });
    tokio::time::timeout(
        Duration::from_secs(1),
        execution.admission_reached.notified(),
    )
    .await
    .expect("first dispatcher must hold the original reservation before admission");
    assert!(store.get_agent_active_turn("s1").await.unwrap().is_none());
    let automatic = service.try_dispatch_next("s1").await;
    let manual = service
        .start_queued_message(test_session(), Some(&message_id))
        .await;
    // Always release the original owner before asserting, including the red run.
    execution.admission_release.notify_one();
    winner.await.unwrap().unwrap().unwrap();
    wait_for_canonical_terminal(store.as_ref()).await;
    assert!(automatic.unwrap().is_none());
    assert!(manual
        .unwrap_err()
        .is_code(talos_error::ErrorCode::MessageQueueBusy));
    assert_eq!(execution.stream_requests.lock().unwrap().len(), 1);
    assert!(queue.list("s1").unwrap().messages.is_empty());
    let events = timeline.list_events_after("s1", None, 100).await.unwrap();
    assert!(!events.iter().any(|record| matches!(
        record.event,
        talos_agent_api::TimelineEventPayload::SystemNotice(_)
    )));
}

#[tokio::test]
async fn automatic_dispatch_preserves_busy_errors_after_reservation() {
    let execution = Arc::new(RecordingAgentExecution {
        stream_error: Mutex::new(Some(TalosError::new(
            talos_error::ErrorCode::MessageQueueBusy,
            "Worker admission is busy",
        ))),
        ..Default::default()
    });
    let (service, _, store, queue) = test_service(execution).await;
    create_test_session_record(store).await;
    queue
        .add("s1", test_user_input("retain admission failure"))
        .unwrap();
    let error = service.try_dispatch_next("s1").await.unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::MessageQueueBusy));
    assert_eq!(queue.list("s1").unwrap().messages.len(), 1);
    // The failed later phase must release its reservation, not strand the row.
    let reservation = queue.reserve_front("s1").unwrap().unwrap();
    queue.release(reservation).unwrap();
}

#[tokio::test]
async fn automation_settlement_warning_preserves_usage_cleanup_and_queued_work() {
    struct UnusedSkills;
    impl crate::automation::goal_loop::AutomationSkillResolver for UnusedSkills {
        fn resolve<'a>(
            &'a self,
            _: &'a SessionRecord,
            _: talos_types::AutomationMode,
        ) -> futures_util::future::BoxFuture<'a, TalosResult<talos_types::SkillSelector>> {
            unreachable!("ordinary queued input does not select an automation Skill")
        }
    }
    let logs = tempfile::tempdir().unwrap();
    let log_path = logs.path().join("automation.log");
    let dispatch = log_dispatch(&log_path);
    let _subscriber = tracing::dispatcher::set_default(&dispatch);
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    let execution = Arc::new(RecordingAgentExecution::default());
    let (service, _, store, queue) = test_service_with_storage(
        execution.clone(),
        Arc::new(RecordingAgentCoordinator::default()),
        storage.clone(),
    )
    .await;
    service
        .install_automation(store.automation_store(), Arc::new(UnusedSkills))
        .unwrap();
    create_test_session_record(store.clone()).await;
    let identity = AgentTurnRef::new("s1", "recovered-turn");
    let input = completed_agent_user_input(
        &identity.turn_id,
        "input-original",
        test_user_input("Original work"),
        1,
    )
    .unwrap();
    commit_test_running_turn(store.as_ref(), identity.clone(), input).await;
    DatabaseProfileStore::new(storage.pool().clone())
        .record_turn_usage(talos_store::profile::TurnUsageUpdate {
            user_id: "u1".into(),
            session_id: identity.session_id.clone(),
            turn_id: identity.turn_id.clone(),
            request_id: "profile-original".into(),
            model_ref: ModelRef {
                provider: "mock".into(),
                id: "model-1".into(),
            },
            usage: Default::default(),
            usage_available: false,
            stop_reason: StopReason::WaitingUserAction,
            timezone: "UTC".into(),
            completed_at_ms: 1000,
            usage_date: "2026-09-06".into(),
        })
        .await
        .unwrap();
    sqlx::query("UPDATE agent_events SET automation_origin_json='invalid-json' WHERE session_id='s1' AND turn_id='recovered-turn' AND sequence=1")
        .execute(storage.pool()).await.unwrap();
    queue
        .add("s1", test_user_input("Next ordinary work"))
        .unwrap();
    let active = store.get_agent_active_turn("s1").await.unwrap().unwrap();
    service
        .recover_active_turn(test_session(), active)
        .await
        .unwrap();
    wait_for_stream_requests(execution.as_ref(), 1).await;
    let remaining_usage: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM profile_turn_usage WHERE session_id='s1' AND turn_id='recovered-turn'",
    )
    .fetch_one(storage.pool())
    .await
    .unwrap();
    assert_eq!(remaining_usage, 0);
    assert!(queue.list("s1").unwrap().messages.is_empty());
    assert!(std::fs::read_to_string(log_path)
        .unwrap()
        .contains("settle Session automation"));
}
