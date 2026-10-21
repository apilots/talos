use super::*;

#[tokio::test]
async fn active_turn_delivery_reports_kernel_acceptance() {
    let (accepted_tx, accepted_rx) = oneshot::channel();
    let delivery = ActiveTurnInputDelivery::new(
        ActiveTurnInput {
            browser_uploads: Vec::new(),
            input: UserInput {
                text: "focus on tests".to_string(),
                file_ids: Vec::new(),
            },
            message: ChatMessage::user("focus on tests"),
            attachment_tokens: 0,
        },
        accepted_tx,
    );

    let input = delivery.acknowledge();

    assert_eq!(input.input.text, "focus on tests");
    assert_eq!(accepted_rx.await, Ok(()));
}

#[tokio::test]
async fn dropped_active_turn_delivery_reports_rejection() {
    let (accepted_tx, accepted_rx) = oneshot::channel();
    let delivery = ActiveTurnInputDelivery::new(
        ActiveTurnInput {
            browser_uploads: Vec::new(),
            input: UserInput::default(),
            message: ChatMessage::user(String::new()),
            attachment_tokens: 0,
        },
        accepted_tx,
    );

    drop(delivery);

    assert!(accepted_rx.await.is_err());
}
