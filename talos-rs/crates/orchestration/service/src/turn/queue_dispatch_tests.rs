use super::*;
use talos_agent_api::AgentTurnRef;

#[tokio::test]
async fn committed_failure_recovery_releases_execution_and_dispatches_the_queue() {
    for rejected_stage in ["permit", "metricsUpdated", "turnCompleted"] {
        let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
        let execution = Arc::new(RecordingAgentExecution::default());
        let (service, _, store, queue) = test_service_with_storage(
            execution.clone(),
            Arc::new(RecordingAgentCoordinator::default()),
            storage.clone(),
        )
        .await;
        create_test_session_record(store.clone()).await;
        if rejected_stage == "metricsUpdated" {
            *execution.progress_event.lock().unwrap() = Some(
                CanonicalAgentEventPayload::MetricsUpdated(AgentRunMetrics::default()),
            );
        }
        if rejected_stage == "permit" {
            *execution.start_permit_error.lock().unwrap() = Some(
                talos_agent::AgentError::provider_error("injected start permit rejection"),
            );
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
                CanonicalAgentEventPayload::TurnFailed(failure) if failure.error.message.contains("injected")
            ))
        );
        assert!(events
            .windows(2)
            .all(|pair| pair[1].sequence == pair[0].sequence + 1));
        assert_eq!(execution.stream_requests.lock().unwrap().len(), 2);
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
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(message_queue.list("s1").unwrap().messages.is_empty());
}
