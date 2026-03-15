use dashmap::DashMap;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Execution context passed to tools.
#[derive(Clone, Debug)]
pub struct ToolContext {
    session_id: String,
    user_id: String,
    state: Arc<DashMap<String, Value>>,
    metadata: Arc<HashMap<String, Value>>,
}

impl ToolContext {
    pub fn new(
        session_id: impl Into<String>,
        user_id: impl Into<String>,
        metadata: HashMap<String, Value>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            user_id: user_id.into(),
            state: Arc::new(DashMap::new()),
            metadata: Arc::new(metadata),
        }
    }

    pub fn anonymous() -> Self {
        Self::new("anonymous-session", "anonymous-user", HashMap::new())
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

    pub fn metadata(&self, key: &str) -> Option<Value> {
        self.metadata.get(key).cloned()
    }

    pub fn metadata_all(&self) -> HashMap<String, Value> {
        self.metadata.as_ref().clone()
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
        let context = ToolContext::new(
            "session-1",
            "user-1",
            HashMap::from([("env".to_string(), serde_json::json!("test"))]),
        );

        context.set_state("cursor", serde_json::json!(10));
        assert_eq!(context.get_state("cursor"), Some(serde_json::json!(10)));

        let removed = context.remove_state("cursor");
        assert_eq!(removed, Some(serde_json::json!(10)));
        assert_eq!(context.get_state("cursor"), None);

        assert_eq!(context.metadata("env"), Some(serde_json::json!("test")));
    }
}
