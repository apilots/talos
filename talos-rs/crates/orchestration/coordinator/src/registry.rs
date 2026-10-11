use std::collections::{hash_map::Entry, HashMap};
use std::sync::Arc;

use talos_agent::{AgentHarness, AgentResult};
use talos_error::TalosError;
use tokio::sync::RwLock;

pub(crate) const EMBEDDED_NODE_ID: &str = "embedded";
type RegisteredHarnesses = HashMap<(String, String), Arc<dyn AgentHarness>>;

/// Coordinator-owned registry of available AgentHarness instances.
///
/// The registry is an orchestration index only. It does not instantiate Agents,
/// write stores, or interpret Provider-private configuration.
#[derive(Clone, Default)]
pub(crate) struct AgentRegistry {
    registrations: Arc<RwLock<RegisteredHarnesses>>,
    environments: Arc<RwLock<HashMap<String, Arc<dyn agent_protocol::WorkerEnvironment>>>>,
}

impl AgentRegistry {
    /// Create an empty Agent registry.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Register a Harness under one stable node and logical Agent id.
    ///
    /// The caller is responsible for constructing the Harness through the
    /// Harness factory before registration. Aliases belong in routing config,
    /// not registry identity.
    pub(crate) async fn register(
        &self,
        node_id: String,
        agent_id: String,
        harness: Arc<dyn AgentHarness>,
    ) -> AgentResult<()> {
        if node_id.trim().is_empty() || agent_id.trim().is_empty() {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "node_id and agent_id cannot be empty",
            ));
        }
        let mut registrations = self.registrations.write().await;
        match registrations.entry((node_id, agent_id.clone())) {
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

    pub(crate) async fn replace(
        &self,
        node_id: String,
        agent_id: String,
        harness: Arc<dyn AgentHarness>,
    ) -> AgentResult<()> {
        if node_id.trim().is_empty() || agent_id.trim().is_empty() {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "node_id and agent_id cannot be empty",
            ));
        }
        self.registrations
            .write()
            .await
            .insert((node_id, agent_id), harness);
        Ok(())
    }

    pub(crate) async fn unregister_node(&self, node_id: &str) {
        self.environments.write().await.remove(node_id);
        self.registrations
            .write()
            .await
            .retain(|(registered_node, _), _| registered_node != node_id);
    }

    pub(crate) async fn register_environment(
        &self,
        node_id: String,
        environment: Arc<dyn agent_protocol::WorkerEnvironment>,
    ) {
        self.environments.write().await.insert(node_id, environment);
    }

    pub(crate) async fn environment(
        &self,
        node_id: &str,
    ) -> AgentResult<Arc<dyn agent_protocol::WorkerEnvironment>> {
        self.environments
            .read()
            .await
            .get(node_id)
            .cloned()
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ServiceUnavailable,
                    "Worker environment is unavailable",
                )
            })
    }

    /// Resolve a Harness by node and logical Agent id.
    pub(crate) async fn get(
        &self,
        node_id: &str,
        agent_id: &str,
    ) -> AgentResult<Arc<dyn AgentHarness>> {
        let registrations = self.registrations.read().await;
        registrations
            .get(&(node_id.to_string(), agent_id.to_string()))
            .cloned()
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    agent_id.to_string(),
                )
            })
    }

    pub(crate) async fn contains_agent(&self, agent_id: &str) -> bool {
        self.registrations
            .read()
            .await
            .keys()
            .any(|(_, registered_agent)| registered_agent == agent_id)
    }

    pub(crate) async fn nodes_for_agent(&self, agent_id: &str) -> Vec<String> {
        let registrations = self.registrations.read().await;
        let mut nodes = registrations
            .keys()
            .filter(|(_, registered_agent)| registered_agent == agent_id)
            .map(|(node_id, _)| node_id.clone())
            .collect::<Vec<_>>();
        nodes.sort();
        nodes
    }

    /// List runtime resources owned by this control-plane process.
    /// Independent nodes retain their lifecycle across control-plane restarts.
    pub(crate) async fn owned_harnesses(&self) -> Vec<Arc<dyn AgentHarness>> {
        let registrations = self.registrations.read().await;
        registrations
            .iter()
            .filter(|((node_id, _), _)| node_id == EMBEDDED_NODE_ID)
            .map(|(_, harness)| harness.clone())
            .collect()
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
