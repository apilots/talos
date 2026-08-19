use super::*;

#[tokio::test]
async fn failed_queue_start_keeps_the_message_in_place() {
    let turn_executor = Arc::new(RecordingTurnExecutor {
        stream_error: Mutex::new(Some(TalosError::public(
            talos_error::ErrorCode::Conflict,
            "agent busy",
        ))),
        ..Default::default()
    });
    let (service, _event_service, store, message_queue) = test_service(turn_executor).await;
    create_test_session_record(store).await;
    let queue = message_queue
        .add(
            "s1",
            vec![TalosInputItem::Text {
                text: "keep me".to_string(),
            }],
        )
        .unwrap();

    let error = service
        .start_queued_message(test_session(), Some(&queue.messages[0].id))
        .await
        .expect_err("failed execution submission must retain the message");

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    assert_eq!(message_queue.list("s1").unwrap().messages, queue.messages);
}
