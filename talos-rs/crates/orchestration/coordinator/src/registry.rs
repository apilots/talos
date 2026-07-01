use std::collections::{hash_map::Entry, HashMap};
use std::sync::Arc;

use talos_agent::{AgentDescriptor, AgentError, AgentErrorKind, AgentHarness, AgentResult};
use tokio::sync::RwLock;

/// Registered Agent entry used by Coordinator routing.
///
/// The registration pairs management metadata with the Harness facade.
/// Coordinator must use the Harness API and must not downcast to concrete
/// runtime implementations.
#[derive(Clone)]
pub struct AgentRegistration {
    /// Descriptor used for routing and management surfaces.
    pub descriptor: AgentDescriptor,
    /// Harness facade owned by the Agent framework layer.
    pub harness: Arc<dyn AgentHarness>,
}

/// Coordinator-owned registry of available AgentHarness instances.
///
/// The registry is an orchestration index only. It does not instantiate Agents,
/// write stores, or interpret runtime-private configuration.
#[derive(Clone, Default)]
pub struct AgentRegistry {
    registrations: Arc<RwLock<HashMap<String, AgentRegistration>>>,
}

impl AgentRegistry {
    /// Create an empty Agent registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a Harness under a stable Agent id.
    ///
    /// The caller is responsible for constructing the Harness through the
    /// Harness factory before registration. The registry key must match the
    /// Harness descriptor id; aliases belong in routing config, not registry
    /// identity.
    pub async fn register(
        &self,
        agent_id: &str,
        harness: Arc<dyn AgentHarness>,
    ) -> AgentResult<()> {
        if agent_id.trim().is_empty() {
            return Err(AgentError::invalid_execution_context(
                "agent_id cannot be empty",
            ));
        }
        let descriptor = harness.descriptor();
        if descriptor.agent_id != agent_id {
            return Err(AgentError::invalid_execution_context(format!(
                "agent registry key `{agent_id}` does not match descriptor id `{}`",
                descriptor.agent_id
            )));
        }
        let mut registrations = self.registrations.write().await;
        match registrations.entry(agent_id.to_string()) {
            Entry::Occupied(_) => Err(AgentError::run_conflict(format!(
                "agent `{agent_id}` is already registered"
            ))),
            Entry::Vacant(entry) => {
                entry.insert(AgentRegistration {
                    descriptor,
                    harness,
                });
                Ok(())
            }
        }
    }

    /// Resolve a registered Agent by id.
    pub async fn get(&self, agent_id: &str) -> AgentResult<AgentRegistration> {
        let registrations = self.registrations.read().await;
        registrations
            .get(agent_id)
            .cloned()
            .ok_or_else(|| AgentError::new(AgentErrorKind::AgentNotFound, agent_id.to_string()))
    }

    /// List registered Agents for routing and management views.
    pub async fn list(&self) -> Vec<AgentRegistration> {
        let registrations = self.registrations.read().await;
        registrations.values().cloned().collect()
    }

    /// Return the current number of registered Agents.
    pub async fn len(&self) -> usize {
        self.registrations.read().await.len()
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
