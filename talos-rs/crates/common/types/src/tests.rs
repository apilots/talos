use super::*;
use std::collections::HashMap;

#[test]
fn test_agent_id() {
    let id = AgentId::new("test");
    assert_eq!(id.0, "test");

    let id2 = AgentId::new_random();
    assert!(!id2.0.is_empty());
}

#[test]
fn test_session_id() {
    let id = SessionId::new();
    assert!(!id.0.is_empty());
}

#[test]
fn test_talos_roundtrip() {
    let req = talos::TalosMessageRequest {
        input: vec![talos::TalosInputItem::Text {
            text: "hello".to_string(),
        }],
        stream: Some(false),
        workspace_id: Some("workspace-1".to_string()),
    };

    let encoded = serde_json::to_string(&req).unwrap();
    let decoded: talos::TalosMessageRequest = serde_json::from_str(&encoded).unwrap();
    assert!(decoded.stream.is_some());
    assert_eq!(decoded.workspace_id.as_deref(), Some("workspace-1"));
    assert!(matches!(
        decoded.input.first(),
        Some(talos::TalosInputItem::Text { text }) if text == "hello"
    ));
}

#[test]
fn test_session_context_and_state_from_record() {
    let record = SessionRecord {
        id: "session-1".to_string(),
        title: "Untitled Session".to_string(),
        context: talos::TalosContext {
            source_type: talos::TalosSourceType::Im,
            provider: talos::TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("channel-1".to_string()),
            thread_id: Some("thread-1".to_string()),
            environment: None,
        },
        model: "qwen-max".to_string(),
        created_at: 100,
        last_activity: 200,
        message_count: 3,
        usage: talos::TalosUsage::default(),
        conf: HashMap::new(),
    };

    let context = SessionContext::from_record(&record, Some("workspace-1".to_string()));
    let state = SessionState::from_record(&record);

    assert_eq!(context.session_id, "session-1");
    assert_eq!(context.workspace_id.as_deref(), Some("workspace-1"));
    assert_eq!(context.provider, talos::TalosProvider::Telegram);
    assert_eq!(state.created_at, 100);
    assert_eq!(state.last_activity, 200);
    assert_eq!(state.message_count, 3);
    assert_eq!(state.usage.total_tokens, 0);
}

#[test]
fn test_talos_session_from_record_applies_conf_override() {
    let mut record = SessionRecord {
        id: "session-1".to_string(),
        title: "Untitled Session".to_string(),
        context: talos::TalosContext {
            source_type: talos::TalosSourceType::Http,
            provider: talos::TalosProvider::Http,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("default".to_string()),
            thread_id: None,
            environment: None,
        },
        model: "qwen-max".to_string(),
        created_at: 100,
        last_activity: 200,
        message_count: 3,
        usage: talos::TalosUsage::default(),
        conf: HashMap::new(),
    };
    record.conf.insert(
        "agent.model".to_string(),
        serde_json::Value::String("qwen-plus".to_string()),
    );

    let session = TalosSession::from_record(&record, &TalosConfig::default(), None)
        .expect("talos session restore should succeed");
    assert_eq!(session.title, "Untitled Session");
    assert_eq!(session.config.agent.model, "qwen-plus");
    assert_eq!(session.config.llm.model, "qwen-plus");
}

#[test]
fn test_session_override_policy_rejects_unknown_keys() {
    let mut conf = HashMap::new();
    conf.insert(
        "agent.kernel.prompt.max_dynamic_tokens".to_string(),
        serde_json::Value::Number(1.into()),
    );

    let error = SessionConfigOverridePolicy::validate_conf(&conf)
        .expect_err("unknown conf key should be rejected");
    assert!(error.to_string().contains("unsupported session config key"));
}

#[test]
fn test_session_override_policy_builds_model_conf() {
    let conf = SessionConfigOverridePolicy::single_model_conf("gpt-4o");
    let model = SessionConfigOverridePolicy::extract_agent_model(&conf)
        .expect("conf extraction should succeed");

    assert_eq!(model.as_deref(), Some("gpt-4o"));
}
