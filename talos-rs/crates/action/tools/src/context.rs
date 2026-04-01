use dashmap::DashMap;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use talos_config::AgentKernelConfig;

/// Execution context passed to tools.
#[derive(Clone, Debug)]
pub struct ToolContext {
    session_id: String,
    user_id: String,
    state: Arc<DashMap<String, Value>>,
    kernel_config: Option<Arc<AgentKernelConfig>>,
    pre_approved: bool,
}

impl ToolContext {
    pub fn new(session_id: impl Into<String>, user_id: impl Into<String>) -> Self {
        Self::new_with_kernel_config(session_id, user_id, None)
    }

    pub fn new_with_kernel_config(
        session_id: impl Into<String>,
        user_id: impl Into<String>,
        kernel_config: Option<Arc<AgentKernelConfig>>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            user_id: user_id.into(),
            state: Arc::new(DashMap::new()),
            kernel_config,
            pre_approved: false,
        }
    }

    pub fn with_pre_approved(mut self) -> Self {
        self.pre_approved = true;
        self
    }

    pub fn pre_approved(&self) -> bool {
        self.pre_approved
    }

    pub fn anonymous() -> Self {
        Self::new("anonymous-session", "anonymous-user")
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn get_state(&self, key: &str) -> Option<Value> {
        self.state.get(key).map(|v| v.value().clone())
    }

    pub fn set_state(&self, key: impl Into<String>, value: Value) {
        self.state.insert(key.into(), value);
    }

    pub fn remove_state(&self, key: &str) -> Option<Value> {
        self.state.remove(key).map(|(_, v)| v)
    }

    pub fn kernel_config(&self) -> Option<&AgentKernelConfig> {
        self.kernel_config.as_deref()
    }

    pub fn state_snapshot(&self) -> HashMap<String, Value> {
        self.state
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }
}

impl Default for ToolContext {
    fn default() -> Self {
        Self::anonymous()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_state_management() {
        let context = ToolContext::new("session-1", "user-1");

        context.set_state("cursor", serde_json::json!(10));
        assert_eq!(context.get_state("cursor"), Some(serde_json::json!(10)));

        let removed = context.remove_state("cursor");
        assert_eq!(removed, Some(serde_json::json!(10)));
        assert_eq!(context.get_state("cursor"), None);
    }
}
