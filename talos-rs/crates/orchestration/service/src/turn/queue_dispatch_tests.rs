use super::*;
use talos_agent_api::AgentTurnRef;

struct TestAutomationSkills;
impl crate::automation::goal_loop::AutomationSkillResolver for TestAutomationSkills {
    fn resolve<'a>(
        &'a self,
        _: &'a SessionRecord,
        mode: talos_types::AutomationMode,
    ) -> futures_util::future::BoxFuture<'a, TalosResult<talos_types::SkillSelector>> {
        Box::pin(async move {
            Ok(talos_types::SkillSelector::selection(
                mode.skill_name(),
                format!("/verified-system/{}/SKILL.md", mode.skill_name()),
            ))
        })
    }
}

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
    let logs = tempfile::tempdir().unwrap();
    let log_path = logs.path().join("profile-discard.log");
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
    create_test_session_record(store.clone()).await;
    let identity = AgentTurnRef::new("s1", "failed-turn");
    let input = completed_agent_user_input("failed-turn", "message-1", test_user_input("first"), 1)
        .unwrap();
    seed_worker_fixture(
        &service,
        store.as_ref(),
        execution.as_ref(),
        canonical_user_admission(identity.clone(), input, 1, true),
    )
    .await;
    fail_worker_fixture(execution.as_ref(), talos_error::ErrorCode::InternalError).await;
    DatabaseProfileStore::new(&storage)
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
    let journal = execution.worker_store.lock().unwrap().clone().unwrap();
    let scope = journal.session_scope("s1").await.unwrap().unwrap();
    let original_settlement = journal.settlement(&identity).await.unwrap().unwrap();
    let original_cursor = journal
        .read_events(agent_protocol::AgentSessionEventsParams {
            session: scope,
            after_cursor: None,
            limit: 256,
        })
        .await
        .unwrap()
        .next_cursor;
    *execution.hold_running_turn.lock().unwrap() = true;
    queue.add("s1", test_user_input("next")).unwrap();
    let active = store.get_agent_active_turn("s1").await.unwrap().unwrap();
    service
        .recover_active_turn(test_session(), active)
        .await
        .unwrap();
    wait_for_stream_requests(execution.as_ref(), 1).await;
    let usage_count = || async {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM profile_turn_usage WHERE session_id='s1' AND turn_id='failed-turn'")
            .fetch_one(storage.pool()).await.unwrap()
    };
    assert_eq!(usage_count().await, 1);
    let receipt = store
        .pending_worker_settlements("s1")
        .await
        .unwrap()
        .into_iter()
        .find(|receipt| receipt.event.identity == identity)
        .expect("failed product effect must retain its receipt");
    assert_eq!(receipt.cursor, original_cursor);
    let discard_notices = |events: Vec<talos_agent_api::TimelineEvent>| {
        events
            .into_iter()
            .filter(|event| event.event.identity() == &identity)
            .filter(|event| {
                matches!(
                    event.event,
                    talos_agent_api::TimelineEventPayload::SystemNotice(_)
                )
            })
            .collect::<Vec<_>>()
    };
    let notices = discard_notices(timeline.list_events_after("s1", None, 100).await.unwrap());
    let [notice] = notices.as_slice() else {
        panic!("one controlled discard failure must publish one notice");
    };
    assert!(
        matches!(&notice.event, talos_agent_api::TimelineEventPayload::SystemNotice(data)
        if data.notice.level == talos_types::TalosTimelineNoticeLevel::Warning
        && matches!(&data.notice.content, talos_agent_api::SystemNoticeContent::Error(error) if error.code == talos_error::ErrorCode::InternalError))
    );
    let safe = serde_json::to_string(notice).unwrap();
    for private in [
        "profile-discard-sentinel",
        "DELETE",
        "profile_turn_usage",
        "backtrace_status",
    ] {
        assert!(
            !safe.contains(private),
            "notice leaked private diagnostics: {safe}"
        );
    }
    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        content.contains("profile-discard-sentinel")
            && content.contains("discard_turn_usage")
            && content.contains("backtrace_status=Captured"),
        "{content}"
    );
    let reports = content
        .lines()
        .filter(|line| {
            line.contains("application failure") && line.contains("profile-discard-sentinel")
        })
        .count();
    assert_eq!(reports, 1, "{content}");
    let next = execution.stream_requests.lock().unwrap()[0].clone();
    assert_ne!(next.context.identity(), identity);
    assert!(matches!(&next.input.blocks[0].content,
        talos_agent_api::AgentMessageBlockContent::UserContent(talos_agent_api::AgentUserContent::Text { text }) if text == "next"));

    sqlx::query("DROP TRIGGER reject_profile_discard")
        .execute(storage.pool())
        .await
        .unwrap();
    service
        .recover_worker_settlements("s1", false)
        .await
        .unwrap();
    assert_eq!(usage_count().await, 0);
    assert!(store
        .pending_worker_settlements("s1")
        .await
        .unwrap()
        .iter()
        .all(|receipt| receipt.cursor != original_cursor));
    assert_eq!(execution.stream_requests.lock().unwrap().len(), 1);
    service
        .recover_worker_settlements("s1", false)
        .await
        .unwrap();
    assert_eq!(execution.stream_requests.lock().unwrap().len(), 1);
    assert_eq!(
        discard_notices(timeline.list_events_after("s1", None, 100).await.unwrap()),
        notices
    );
    assert_eq!(
        journal.settlement(&identity).await.unwrap().unwrap(),
        original_settlement
    );
    let retried_log = std::fs::read_to_string(&log_path).unwrap();
    assert_eq!(
        retried_log
            .lines()
            .filter(|line| line.contains("application failure")
                && line.contains("profile-discard-sentinel"))
            .count(),
        reports
    );

    complete_held_turn(&service, execution.as_ref()).await;
}

#[tokio::test]
async fn committed_failure_recovery_releases_execution_and_dispatches_the_queue() {
    enum StartFailure {
        Panic,
        Permit,
    }
    for rejected_stage in [StartFailure::Panic, StartFailure::Permit] {
        let logs = tempfile::tempdir().unwrap();
        let log_path = logs.path().join("recovery.log");
        let dispatch = log_dispatch(&log_path);
        let subscriber = tracing::dispatcher::set_default(&dispatch);
        let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
        let execution = Arc::new(RecordingAgentExecution::default());
        let coordinator = Arc::new(RecordingAgentCoordinator::default());
        let (service, _, store, queue) =
            test_service_with_storage(execution.clone(), coordinator, storage.clone()).await;
        create_test_session_record(store.clone()).await;
        match rejected_stage {
            StartFailure::Panic => execution.panic_start_permit.store(true, Ordering::SeqCst),
            StartFailure::Permit => {
                *execution.start_permit_error.lock().unwrap() = Some(TalosError::new(
                    talos_error::ErrorCode::InternalError,
                    "injected start permit rejection",
                ));
            }
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
        .install_automation(store.automation_store(), Arc::new(TestAutomationSkills))
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
    seed_worker_fixture(
        &service,
        store.as_ref(),
        execution.as_ref(),
        canonical_user_admission(identity.clone(), input, 1, true),
    )
    .await;
    fail_worker_fixture(execution.as_ref(), talos_error::ErrorCode::InternalError).await;
    DatabaseProfileStore::new(&storage)
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

#[tokio::test]
async fn explicit_automation_without_registration_keeps_completion_and_publishes_notice() {
    for mode in [
        talos_types::AutomationMode::Goal,
        talos_types::AutomationMode::Loop,
    ] {
        let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
        let execution = Arc::new(RecordingAgentExecution::default());
        let (service, timeline, store, queue) = test_service_with_storage(
            execution.clone(),
            Arc::new(RecordingAgentCoordinator::default()),
            storage,
        )
        .await;
        service
            .install_automation(store.automation_store(), Arc::new(TestAutomationSkills))
            .unwrap();
        create_test_session_record(store.clone()).await;
        let mut input = test_user_input("Register this task");
        input.content.push(AgentUserContent::SkillSelection {
            name: mode.skill_name().into(),
            path: format!("/verified-system/{}/SKILL.md", mode.skill_name()),
        });
        queue.add("s1", input).unwrap();
        queue
            .add("s1", test_user_input("Next ordinary work"))
            .unwrap();
        let first = service.try_dispatch_next("s1").await.unwrap().unwrap();
        wait_for_stream_requests(execution.as_ref(), 2).await;
        service
            .recover_worker_settlements("s1", false)
            .await
            .unwrap();
        service
            .recover_worker_settlements("s1", false)
            .await
            .unwrap();
        assert!(store
            .pending_worker_settlements("s1")
            .await
            .unwrap()
            .iter()
            .all(|fact| fact.event.identity.turn_id != first.turn_id));

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
        assert_eq!(turn.notices.len(), 1);
        let notice = &turn.notices[0];
        let talos_agent_api::TimelineEventPayload::SystemNotice(data) = &notice.event else {
            unreachable!()
        };
        let talos_agent_api::SystemNoticeContent::Error(error) = &data.notice.content else {
            unreachable!()
        };
        assert_eq!(error.code, talos_error::ErrorCode::ToolExecutionFailed);
        assert_eq!(
            error.message,
            match mode {
                talos_types::AutomationMode::Goal => "No Goal was created in this Turn.",
                talos_types::AutomationMode::Loop => "No Loop was created in this Turn.",
            }
        );
        assert!(timeline
            .list_events_after("s1", None, 100)
            .await
            .unwrap()
            .contains(notice));
        let snapshot = store.automation_store().snapshot("s1").await.unwrap();
        assert!(snapshot.goal.is_none() && snapshot.loops.is_empty());
    }
}

#[tokio::test]
async fn product_transaction_failure_retains_original_worker_receipt_until_cursor_replay() {
    // Failed transactions close their connection; keep the durable database alive.
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("projection.sqlite");
    let storage = Arc::new(
        SqliteStorage::new(database.to_str().unwrap())
            .await
            .unwrap(),
    );
    let execution = Arc::new(RecordingAgentExecution::default());
    let (service, _, store, queue) = test_service_with_storage(
        execution.clone(),
        Arc::new(RecordingAgentCoordinator::default()),
        storage.clone(),
    )
    .await;
    create_test_session_record(store.clone()).await;
    sqlx::query("CREATE TRIGGER reject_projection BEFORE INSERT ON agent_events BEGIN SELECT RAISE(FAIL,'projection-unavailable'); END").execute(storage.pool()).await.unwrap();
    queue
        .add("s1", test_user_input("queued once after replay"))
        .unwrap();
    let request = StartTurnRequest {
        node_id: None,
        session_id: "s1".into(),
        input: test_user_input("admit once"),
        model_selection: None,
        user_permission_mode: None,
        project_id: None,
        current_directory: None,
        client_request_id: Some("durable-original".into()),
    };
    let first = submit_test_start(
        &service,
        test_session(),
        request.clone(),
        Some("durable-original"),
    )
    .await
    .unwrap();
    let journal = execution.worker_store.lock().unwrap().clone().unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if journal
                .settlement(&AgentTurnRef::new("s1", &first.turn_id))
                .await
                .unwrap()
                .is_some()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(execution.stream_requests.lock().unwrap().len(), 1);
    assert!(service
        .reserve_start_turn(&request, Some("durable-original"), Duration::ZERO)
        .await
        .is_err());
    assert_eq!(execution.stream_requests.lock().unwrap().len(), 1);
    sqlx::query("DROP TRIGGER reject_projection")
        .execute(storage.pool())
        .await
        .unwrap();
    service.reconcile_worker_events("s1").await.unwrap();
    wait_for_stream_requests(execution.as_ref(), 2).await;
    let replay = submit_test_start(&service, test_session(), request, Some("durable-original"))
        .await
        .unwrap();
    assert_eq!(replay.turn_id, first.turn_id);
    assert_eq!(replay.request_id, first.request_id);
    assert_eq!(execution.stream_requests.lock().unwrap().len(), 2);
    let facts = journal.terminal_events("s1", None, 256).await.unwrap();
    assert!(facts
        .iter()
        .any(|(_, event)| event.identity.turn_id == first.turn_id
            && matches!(event.payload, CanonicalAgentEventPayload::TurnCompleted(_))));
}

#[tokio::test]
async fn lost_commit_confirmation_retains_queue_identity_until_worker_replay() {
    let execution = Arc::new(RecordingAgentExecution::default());
    *execution.admission_fault.lock().unwrap() = crate::test_worker::AdmissionFault::AfterCommit;
    let (service, _, store, queue) = test_service(execution.clone()).await;
    create_test_session_record(store.clone()).await;
    let first = queue
        .add("s1", test_user_input("original queued input"))
        .unwrap()
        .messages[0]
        .id
        .clone();
    queue
        .add("s1", test_user_input("next queued input"))
        .unwrap();
    let error = service.try_dispatch_next("s1").await.unwrap_err();
    assert!(agent_protocol::AgentAdmissionUnconfirmed::contains(&error));
    assert!(!queue.is_reserved(&first, "s1").unwrap());
    let original = execution.stream_requests.lock().unwrap()[0].clone();
    let original_key = original.command.as_ref().unwrap().idempotency_key.clone();
    assert_eq!(original_key, format!("queued:{first}"));
    let journal = execution.worker_store.lock().unwrap().clone().unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while journal
            .settlement(&original.context.identity())
            .await
            .unwrap()
            .is_none()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    service.reconcile_worker_events("s1").await.unwrap();
    wait_for_stream_requests(&execution, 2).await;
    assert!(!queue
        .list("s1")
        .unwrap()
        .messages
        .iter()
        .any(|message| message.id == first));
    assert_eq!(
        execution
            .stream_requests
            .lock()
            .unwrap()
            .iter()
            .filter(|params| params
                .command
                .as_ref()
                .is_some_and(|command| command.idempotency_key == original_key))
            .count(),
        1
    );
}

#[tokio::test]
async fn unknown_commit_not_received_retries_original_queue_identity_after_lease_expiry() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    let execution = Arc::new(RecordingAgentExecution::default());
    *execution.admission_fault.lock().unwrap() = crate::test_worker::AdmissionFault::BeforeCommit;
    let (service, _, store, queue) = test_service_with_storage(
        execution.clone(),
        Arc::new(RecordingAgentCoordinator::default()),
        storage.clone(),
    )
    .await;
    create_test_session_record(store.clone()).await;
    let message_id = queue
        .add("s1", test_user_input("original input"))
        .unwrap()
        .messages[0]
        .id
        .clone();
    let error = service.try_dispatch_next("s1").await.unwrap_err();
    assert!(agent_protocol::AgentAdmissionUnconfirmed::contains(&error));
    assert!(!queue.is_reserved(&message_id, "s1").unwrap());
    let original = execution.stream_requests.lock().unwrap()[0].clone();
    let journal = execution.worker_store.lock().unwrap().clone().unwrap();
    assert!(journal
        .execution_state(&original.context.session)
        .await
        .unwrap()
        .is_none());
    // The existing product command lease governs uncertainty; reclaims keep original identities.
    sqlx::query("UPDATE agent_command_idempotency SET lease_expires_at=1 WHERE status='reserved'")
        .execute(storage.pool())
        .await
        .unwrap();
    let accepted = service.try_dispatch_next("s1").await.unwrap().unwrap();
    assert_eq!(accepted.turn_id, original.context.turn_id);
    assert_eq!(accepted.message_id.as_deref(), Some(message_id.as_str()));
    let retry = execution.stream_requests.lock().unwrap()[1].clone();
    assert_eq!(retry.command, original.command);
    wait_for_canonical_terminal(store.as_ref()).await;
    assert_eq!(
        journal
            .terminal_events("s1", None, 256)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn expired_unknown_command_replays_worker_admission_before_rebuilding_input() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    let execution = Arc::new(RecordingAgentExecution::default());
    let (service, _, store, _) = test_service_with_storage(
        execution.clone(),
        Arc::new(RecordingAgentCoordinator::default()),
        storage.clone(),
    )
    .await;
    *execution.admission_fault.lock().unwrap() = crate::test_worker::AdmissionFault::AfterCommit;
    create_test_session_record(store.clone()).await;
    let request = StartTurnRequest {
        node_id: None,
        session_id: "s1".into(),
        input: test_user_input("same original request"),
        model_selection: None,
        user_permission_mode: None,
        project_id: None,
        current_directory: None,
        client_request_id: Some("expired-unknown".into()),
    };
    let error = submit_test_start(
        &service,
        test_session(),
        request.clone(),
        Some("expired-unknown"),
    )
    .await
    .unwrap_err();
    assert!(agent_protocol::AgentAdmissionUnconfirmed::contains(&error));
    let original = execution.stream_requests.lock().unwrap()[0].clone();
    let journal = execution.worker_store.lock().unwrap().clone().unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while journal
            .settlement(&original.context.identity())
            .await
            .unwrap()
            .is_none()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    sqlx::query("UPDATE agent_command_idempotency SET lease_expires_at=1 WHERE status='reserved'")
        .execute(storage.pool())
        .await
        .unwrap();
    // No background reconcile ran: this retry itself must find the original Worker receipt.
    let replay = submit_test_start(&service, test_session(), request, Some("expired-unknown"))
        .await
        .unwrap();
    assert_eq!(replay.turn_id, original.context.turn_id);
    assert_eq!(
        replay.message_id.as_deref(),
        Some(original.input.message_id.as_str())
    );
    assert_eq!(execution.stream_requests.lock().unwrap().len(), 1);
}
