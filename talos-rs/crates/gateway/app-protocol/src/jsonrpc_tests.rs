use super::*;
use serde_json::json;

#[test]
fn request_roundtrips() {
    let request = JsonRpcRequest::new(
        RequestId::Number(1),
        "session/get",
        Some(json!({"session_id": "s1"})),
    );
    let value = serde_json::to_string(&request).unwrap();
    let decoded: JsonRpcMessage = serde_json::from_str(&value).unwrap();
    assert!(matches!(decoded, JsonRpcMessage::Request(_)));
}

#[test]
fn malformed_json_maps_to_parse_error() {
    let error = parse_message_line("{").unwrap_err();
    assert_eq!(error.code, crate::errors::PARSE_ERROR);
}

#[test]
fn notification_roundtrips() {
    let notification = JsonRpcNotification::new("initialized", None);
    let value = serde_json::to_string(&notification).unwrap();
    let decoded: JsonRpcMessage = serde_json::from_str(&value).unwrap();
    assert!(matches!(decoded, JsonRpcMessage::Notification(_)));
}
