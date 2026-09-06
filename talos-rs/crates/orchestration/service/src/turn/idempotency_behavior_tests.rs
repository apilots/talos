use super::*;

#[tokio::test]
async fn concurrent_idempotent_submission_does_not_start_a_second_execution() {
    let agent_execution = Arc::new(RecordingAgentExecution::default());
    let (service, _event_service, store, _message_queue) =
        test_service(agent_execution.clone()).await;
    create_test_session_record(store.clone()).await;
    let request = StartTurnRequest {
        session_id: "s1".to_string(),
        input: test_user_input("hello"),
        model_selection: None,
        user_permission_mode: None,
        project_id: None,
        current_directory: None,
        client_request_id: None,
    };
    let request_hash = start_turn_hash(&request).unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    store
        .reserve_command(CommandReservation {
            session_id: "s1".to_string(),
            command: IdempotentCommand::TurnSubmission,
            idempotency_key: "in-progress".to_string(),
            request_hash,
            request_id: "request-existing".to_string(),
            turn_id: "turn-existing".to_string(),
            message_id: "message-existing".to_string(),
            lease_id: "lease-existing".to_string(),
            now,
            lease_expires_at: now.saturating_add(30_000),
            record_expires_at: None,
        })
        .await
        .expect("idempotency reservation should be inserted");

    let error = submit_test_start(&service, test_session(), request, Some("in-progress"))
        .await
        .expect_err("concurrent idempotent submission should report a conflict");

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    assert!(agent_execution.stream_request.lock().unwrap().is_none());
}
