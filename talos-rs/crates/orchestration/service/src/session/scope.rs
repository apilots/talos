use talos_types::talos::TalosContext;

pub const SESSION_SCOPE_MAIN: &str = "main";
pub const SESSION_SCOPE_PER_CHANNEL_PEER: &str = "per_channel_peer";
pub const SESSION_SCOPE_PER_CHANNEL_PEER_TOPIC: &str = "per_channel_peer_topic";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionScope {
    Main,
    PerChannelPeer,
    #[default]
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
#[path = "scope_tests.rs"]
mod tests;
