use super::*;
fn command() -> BrowserHostRequest {
    BrowserHostRequest {
        generation: Uuid::new_v4().to_string(),
        epoch: 1,
        user_id: "alice".into(),
        target: BrowserTargetKind::Managed,
        operation: BrowserHostOperation::Show,
    }
}
#[tokio::test]
async fn host_response_is_correlated_and_consumed_once() {
    let bridge = DesktopHostBridge::new();
    bridge.activate();
    let mut receiver = bridge.take_receiver().expect("receiver");
    let pending = tokio::spawn({
        let bridge = bridge.clone();
        async move { bridge.browser_request(command()).await }
    });
    let DesktopHostEvent::Execute { id, .. } = receiver.recv().await.expect("request") else {
        panic!("execute")
    };
    bridge.complete("unrelated", Ok(serde_json::json!({"kind":"done"})));
    assert!(!pending.is_finished());
    bridge.complete(&id, Ok(serde_json::json!({"kind":"done"})));
    assert_eq!(
        pending.await.expect("join").expect("response"),
        BrowserHostResponse::Done
    );
    bridge.complete(&id, Err(unavailable("late response")));
    assert!(bridge.pending.lock().expect("lock").is_empty());
}
#[tokio::test]
async fn cancelled_host_request_emits_cancel_and_disconnect_fails_pending() {
    let bridge = DesktopHostBridge::new();
    bridge.activate();
    let mut receiver = bridge.take_receiver().expect("receiver");
    let pending = tokio::spawn({
        let bridge = bridge.clone();
        async move { bridge.browser_request(command()).await }
    });
    let DesktopHostEvent::Execute { id, .. } = receiver.recv().await.expect("request") else {
        panic!("execute")
    };
    pending.abort();
    let _ = pending.await;
    let DesktopHostEvent::Cancel { id: cancelled } = receiver.recv().await.expect("cancel") else {
        panic!("cancel")
    };
    assert_eq!(id, cancelled);
    bridge.complete(&id, Ok(serde_json::json!({"kind":"done"})));
    let pending = tokio::spawn({
        let bridge = bridge.clone();
        async move { bridge.browser_request(command()).await }
    });
    receiver.recv().await.expect("request");
    bridge.close();
    assert!(pending.await.expect("join").is_err());
    assert!(bridge.browser_request(command()).await.is_err());
}
