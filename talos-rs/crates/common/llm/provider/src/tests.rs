use super::*;

#[test]
fn test_chat_message() {
    let msg = ChatMessage::user("hello");
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.content, "hello");
}

#[test]
fn test_chat_request_builder() {
    let req = ChatRequest::builder()
        .message("hello")
        .model("claude-3-5-sonnet")
        .temperature(0.7)
        .build();

    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.model, "claude-3-5-sonnet");
    assert_eq!(req.temperature, Some(0.7));
}

#[test]
fn test_provider_config() {
    let config = ProviderConfig::anthropic("test-key");
    assert_eq!(config.name, "anthropic");
    assert!(config.base_url.contains("anthropic.com"));
}

#[test]
fn test_function_call_roundtrip() {
    let call = FunctionCall::new("call_1", "read", serde_json::json!({"path": "/tmp/a"}));
    let encoded = serde_json::to_string(&call).unwrap();
    let decoded: FunctionCall = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, call);
}

#[test]
fn test_function_response_error() {
    let response = FunctionResponse::error("call_1", "read", serde_json::json!("failed"));
    assert!(response.is_error);
}

#[test]
fn test_tool_definition_validation() {
    let def = ToolDefinition::new(
        "read",
        "Read file",
        Some(serde_json::json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {"type": "string"}
            }
        })),
    );

    assert!(def
        .validate_arguments(&serde_json::json!({"path": "/tmp/a"}))
        .is_ok());
    assert!(def.validate_arguments(&serde_json::json!({})).is_err());
}

#[test]
fn test_chat_response_tool_calls_helper() {
    let response = ChatResponse {
        content: String::new(),
        reasoning: Vec::new(),
        model: "mock".to_string(),
        usage: Usage::default(),
        finish_reason: Some("tool_calls".to_string()),
        tool_calls: Some(vec![FunctionCall::new("1", "read", serde_json::json!({}))]),
    };

    assert!(response.has_tool_calls());
    assert_eq!(response.get_tool_calls().len(), 1);
}
