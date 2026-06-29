use super::*;

#[test]
fn test_webhook_manager() {
    let manager = WebhookManager::new();
    assert!(manager.account_ids().is_empty());
}

#[test]
fn test_build_session_id() {
    let sid = build_session_id("123456", None, "user1");
    assert_eq!(sid.0, "telegram:123456:user1");

    let sid = build_session_id("123456", Some("789"), "user1");
    assert_eq!(sid.0, "telegram:123456:789:user1");
}
