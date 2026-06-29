use super::*;
use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType};

fn context() -> TalosContext {
    TalosContext {
        source_type: TalosSourceType::Im,
        provider: TalosProvider::Telegram,
        user_id: "user-1".to_string(),
        message_id: "message-1".to_string(),
        trace_id: "trace-1".to_string(),
        channel_id: Some("group-1".to_string()),
        thread_id: Some("topic-1".to_string()),
        project_root: None,
        environment: None,
    }
}

#[test]
fn resolve_dm_scope_defaults_blank_values() {
    assert_eq!(
        resolve_dm_scope(None),
        SESSION_SCOPE_PER_CHANNEL_PEER_TOPIC.to_string()
    );
    assert_eq!(
        resolve_dm_scope(Some("")),
        SESSION_SCOPE_PER_CHANNEL_PEER_TOPIC.to_string()
    );
    assert_eq!(
        resolve_dm_scope(Some("   ")),
        SESSION_SCOPE_PER_CHANNEL_PEER_TOPIC.to_string()
    );
}

#[test]
fn stable_session_key_respects_main_scope() {
    let key = stable_session_key(&context(), "agent-a", SESSION_SCOPE_MAIN);
    assert!(!key.contains("channel:"));
    assert!(!key.contains("thread:"));
    assert!(key.contains("user:user-1"));
}

#[test]
fn stable_session_key_respects_per_channel_peer_scope() {
    let key = stable_session_key(&context(), "agent-a", SESSION_SCOPE_PER_CHANNEL_PEER);
    assert!(key.contains("channel:group-1"));
    assert!(!key.contains("thread:"));
}

#[test]
fn stable_session_key_defaults_unknown_scope_to_topic_granularity() {
    let key = stable_session_key(&context(), "agent-a", "unknown");
    assert!(key.contains("channel:group-1"));
    assert!(key.contains("thread:topic-1"));
}
