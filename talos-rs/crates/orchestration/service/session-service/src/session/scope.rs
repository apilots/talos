use talos_types::talos::TalosContext;

pub const SESSION_SCOPE_MAIN: &str = "main";
pub const SESSION_SCOPE_PER_CHANNEL_PEER: &str = "per_channel_peer";
pub const SESSION_SCOPE_PER_CHANNEL_PEER_TOPIC: &str = "per_channel_peer_topic";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionScope {
    Main,
    PerChannelPeer,
    PerChannelPeerTopic,
}

impl SessionScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Main => SESSION_SCOPE_MAIN,
            Self::PerChannelPeer => SESSION_SCOPE_PER_CHANNEL_PEER,
            Self::PerChannelPeerTopic => SESSION_SCOPE_PER_CHANNEL_PEER_TOPIC,
        }
    }
}

impl Default for SessionScope {
    fn default() -> Self {
        Self::PerChannelPeerTopic
    }
}

pub fn resolve_dm_scope(dm_scope: Option<&str>) -> String {
    dm_scope
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(SessionScope::default().as_str())
        .to_string()
}

pub fn stable_session_key(context: &TalosContext, agent_id: &str, dm_scope: &str) -> String {
    let thread = context
        .thread_id
        .clone()
        .unwrap_or_else(|| "_main".to_string());
    match dm_scope {
        SESSION_SCOPE_MAIN => format!(
            "agent:{}:provider:{}:user:{}",
            agent_id, context.provider, context.user_id
        ),
        SESSION_SCOPE_PER_CHANNEL_PEER => format!(
            "agent:{}:provider:{}:channel:{}:user:{}",
            agent_id,
            context.provider,
            context.channel_id.as_deref().unwrap_or("_default"),
            context.user_id
        ),
        _ => format!(
            "agent:{}:provider:{}:channel:{}:user:{}:thread:{}",
            agent_id,
            context.provider,
            context.channel_id.as_deref().unwrap_or("_default"),
            context.user_id,
            thread
        ),
    }
}

#[cfg(test)]
mod tests {
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
}
