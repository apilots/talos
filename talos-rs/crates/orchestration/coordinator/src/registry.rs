use std::collections::{hash_map::Entry, HashMap};
use std::sync::Arc;

use talos_agent::{AgentHarness, AgentResult};
use talos_error::TalosError;
use tokio::sync::RwLock;

/// Coordinator-owned registry of available AgentHarness instances.
///
/// The registry is an orchestration index only. It does not instantiate Agents,
/// write stores, or interpret Provider-private configuration.
#[derive(Clone, Default)]
pub(crate) struct AgentRegistry {
    registrations: Arc<RwLock<HashMap<String, Arc<dyn AgentHarness>>>>,
}

impl AgentRegistry {
    /// Create an empty Agent registry.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Register a Harness under one stable Agent id.
    ///
    /// The caller is responsible for constructing the Harness through the
    /// Harness factory before registration. Aliases belong in routing config,
    /// not registry identity.
    pub(crate) async fn register(
        &self,
        agent_id: String,
        harness: Arc<dyn AgentHarness>,
    ) -> AgentResult<()> {
        if agent_id.trim().is_empty() {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "agent_id cannot be empty",
            ));
        }
        let mut registrations = self.registrations.write().await;
        match registrations.entry(agent_id.clone()) {
            Entry::Occupied(_) => Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                format!("agent `{agent_id}` is already registered"),
            )),
            Entry::Vacant(entry) => {
                entry.insert(harness);
                Ok(())
            }
        }
    }

    /// Resolve a registered Agent by id.
    pub(crate) async fn get(&self, agent_id: &str) -> AgentResult<Arc<dyn AgentHarness>> {
        let registrations = self.registrations.read().await;
        registrations.get(agent_id).cloned().ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                agent_id.to_string(),
            )
        })
    }

    /// List Harnesses for Coordinator shutdown.
    pub(crate) async fn list(&self) -> Vec<Arc<dyn AgentHarness>> {
        let registrations = self.registrations.read().await;
        registrations.values().cloned().collect()
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
