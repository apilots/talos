use super::*;

#[tokio::test]
async fn submit_start_turn_records_completed_turn_progress() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    let (service, _event_service, store, _message_queue) =
        test_service(agent_execution.clone()).await;
    create_test_session_record(store.clone()).await;

    submit_test_start(
        &service,
        test_session(),
        StartTurnRequest {
            session_id: "s1".to_string(),
            input: test_user_input("hello"),
            model_selection: None,
            user_permission_mode: None,
            project_id: None,
            current_directory: None,
            client_request_id: None,
        },
        None,
    )
    .await
    .expect("message should be submitted");

    assert!(matches!(
        agent_execution
            .stream_request
            .lock()
            .unwrap()
            .as_ref()
            .map(|request| request.context.session.agent_id.as_str()),
        Some("default")
    ));
    let record = loop {
        let record = store
            .get_session_record("s1")
            .await
            .expect("session record should be readable")
            .expect("session record should exist");
        if record.latest_turn_seq == 1 {
            break record;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert_eq!((record.message_count, record.latest_turn_seq), (2, 1));
}

#[tokio::test]
async fn cancelled_turn_does_not_advance_session_progress() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    agent_execution
        .completed_stop_reason
        .lock()
        .unwrap()
        .replace(StopReason::Cancelled);
    let (service, _event_service, store, _message_queue) = test_service(agent_execution).await;
    create_test_session_record(store.clone()).await;

    submit_test_start(
        &service,
        test_session(),
        StartTurnRequest {
            session_id: "s1".to_string(),
            input: test_user_input("hello"),
            model_selection: None,
            user_permission_mode: None,
            project_id: None,
            current_directory: None,
            client_request_id: None,
        },
        None,
    )
    .await
    .expect("cancelled turn should complete its command lifecycle");

    let record = store
        .get_session_record("s1")
        .await
        .expect("session record should be readable")
        .expect("session record should exist");
    assert_eq!((record.message_count, record.latest_turn_seq), (0, 0));
}
