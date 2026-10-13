use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_protocol::{NodeHealth, NodeMode, NodeStatus};
use async_trait::async_trait;
use talos_agent::{
    AgentAdmission, AgentContext, AgentHarness, AgentInputParams, AgentInterruptParams,
    AgentRunExecution, AgentRunParams, AgentSessionContext,
};
use talos_agent_api::AgentInterruptOutcome;
use talos_error::{TalosError, TalosResult};
use talos_router::Router;
use talos_store::node::AgentNodeStore;
use talos_store::session::SessionStore;

use crate::registry::EMBEDDED_NODE_ID;
use crate::{AgentCoordinator, AgentRegistry, AgentRouter, AgentSelectionParams};

#[cfg(test)]
const HEARTBEAT_TIMEOUT_SECONDS: i64 = 30;

/// Default Coordinator that routes stable Agent bindings to registered Harnesses.
pub struct CoordinatorImpl {
    registry: AgentRegistry,
    agent_router: AgentRouter,
    session_store: Arc<dyn SessionStore>,
    node_store: Arc<dyn AgentNodeStore>,
}

impl CoordinatorImpl {
    /// Create an empty Coordinator using the configured new-Session router.
    pub fn new(
        router: Arc<dyn Router>,
        session_store: Arc<dyn SessionStore>,
        node_store: Arc<dyn AgentNodeStore>,
    ) -> Self {
        let registry = AgentRegistry::new();
        let agent_router = AgentRouter::new(router, registry.clone());
        Self {
            registry,
            agent_router,
            session_store,
            node_store,
        }
    }

    /// Register one already-constructed Harness at application composition.
    pub async fn register_agent(
        &self,
        node_id: String,
        agent_id: String,
        harness: Arc<dyn AgentHarness>,
    ) -> TalosResult<()> {
        self.registry.register(node_id, agent_id, harness).await
    }

    /// Publish a fresh remote lease for one deployment. Re-registration
    /// replaces only the same node/deployment key.
    pub async fn replace_remote_agent(
        &self,
        node_id: String,
        agent_id: String,
        harness: Arc<dyn AgentHarness>,
    ) -> TalosResult<()> {
        self.registry.replace(node_id, agent_id, harness).await
    }

    /// Publish the environment alongside this node's existing deployment handles.
    pub async fn register_environment(
        &self,
        node_id: String,
        environment: Arc<dyn agent_protocol::WorkerEnvironment>,
    ) {
        self.registry
            .register_environment(node_id, environment)
            .await;
    }

    pub async fn unregister_remote_node(&self, node_id: &str) {
        if node_id != "embedded" {
            self.registry.unregister_node(node_id).await;
        }
    }

    async fn harness(
        &self,
        agent_id: &str,
        session_id: &str,
        user_id: Option<&str>,
    ) -> TalosResult<Arc<dyn AgentHarness>> {
        if agent_id.trim().is_empty() {
            return Err(TalosError::internal("Agent binding cannot be empty"));
        }
        let record = self
            .session_store
            .get_session_record(session_id)
            .await?
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::SessionNotFound,
                    "Agent Session is not found",
                )
            })?;
        let namespace = self
            .session_store
            .get_workspace_user_id(session_id)
            .await?
            .unwrap_or_else(|| record.user_id.clone());
        if record.agent_id != agent_id || user_id.is_some_and(|user_id| user_id != namespace) {
            return Err(TalosError::new(
                talos_error::ErrorCode::PermissionDenied,
                "Agent Session binding does not match request",
            ));
        }
        if record.execution_node_id != EMBEDDED_NODE_ID {
            let now = chrono::Utc::now().timestamp();
            let node = self
                .node_store
                .get(&record.execution_node_id)
                .await?
                .ok_or_else(remote_unavailable)?;
            if node.mode != NodeMode::Remote
                || node.status != NodeStatus::Enabled
                || node.owner_user_id.as_deref() != Some(record.user_id.as_str())
                || (node.credential_hash.is_some()
                    && (node.lease_expires_at.is_none_or(|expiry| expiry <= now)
                        || !node_has_recent_heartbeat(&node, now)))
            {
                return Err(remote_unavailable());
            }
        }
        match self.registry.get(&record.execution_node_id, agent_id).await {
            Ok(harness) => Ok(harness),
            Err(error)
                if record.execution_node_id != EMBEDDED_NODE_ID
                    && error.is_code(talos_error::ErrorCode::ResourceNotFound) =>
            {
                Err(remote_unavailable())
            }
            Err(error) => Err(error),
        }
    }

    async fn validate_environment_node(&self, node_id: &str, user_id: &str) -> TalosResult<()> {
        if node_id.trim().is_empty() {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Node ID cannot be empty",
            ));
        }
        let node = self.node_store.get(node_id).await?.ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                "Worker node was not found",
            )
        })?;
        if node_id != EMBEDDED_NODE_ID && node.owner_user_id.as_deref() != Some(user_id) {
            return Err(TalosError::authorization_denied());
        }
        let now = chrono::Utc::now().timestamp();
        if node.status != NodeStatus::Enabled
            || (node_id != EMBEDDED_NODE_ID
                && node.credential_hash.is_some()
                && (node.lease_expires_at.is_none_or(|expiry| expiry <= now)
                    || !node_has_recent_heartbeat(&node, now)))
        {
            return Err(remote_unavailable());
        }
        Ok(())
    }

    async fn context_harness(&self, context: &AgentContext) -> TalosResult<Arc<dyn AgentHarness>> {
        self.harness(
            &context.session.agent_id,
            &context.session.session_id,
            Some(&context.session.workspace.user_id),
        )
        .await
    }
}

fn remote_unavailable() -> TalosError {
    TalosError::new(
        talos_error::ErrorCode::ServiceUnavailable,
        "Bound Agent execution node is unavailable",
    )
}

#[async_trait]
impl AgentCoordinator for CoordinatorImpl {
    async fn read_session_execution_state(
        &self,
        session: AgentSessionContext,
    ) -> TalosResult<Option<agent_protocol::SessionExecutionState>> {
        let harness = self.session_harness(&session).await?;
        harness.read_session_execution_state(session).await
    }
    async fn commit_session_execution_state(
        &self,
        params: agent_protocol::CommitSessionExecutionStateParams,
    ) -> TalosResult<()> {
        let harness = self.session_harness(&params.session).await?;
        harness.commit_session_execution_state(params).await
    }

    async fn worker_environment(
        &self,
        node_id: &str,
        user_id: &str,
    ) -> TalosResult<Arc<dyn agent_protocol::WorkerEnvironment>> {
        self.validate_environment_node(node_id, user_id).await?;
        self.registry.environment(node_id).await
    }

    async fn session_harness(
        &self,
        context: &AgentSessionContext,
    ) -> TalosResult<Arc<dyn AgentHarness>> {
        self.harness(
            &context.agent_id,
            &context.session_id,
            Some(&context.workspace.user_id),
        )
        .await
    }

    async fn resolve_agent(&self, params: AgentSelectionParams) -> TalosResult<String> {
        self.agent_router.resolve_agent(params).await
    }

    async fn ensure_agent(&self, agent_id: &str) -> TalosResult<()> {
        if self.registry.contains_agent(agent_id).await {
            Ok(())
        } else {
            Err(TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                agent_id.to_string(),
            ))
        }
    }

    async fn select_execution_node(
        &self,
        agent_id: &str,
        user_id: &str,
        preferred_node_id: Option<&str>,
    ) -> TalosResult<String> {
        if let Some(node_id) = preferred_node_id {
            self.validate_environment_node(node_id, user_id).await?;
            self.registry
                .get(node_id, agent_id)
                .await
                .map_err(|error| {
                    if error.is_code(talos_error::ErrorCode::ResourceNotFound) {
                        TalosError::new(
                            talos_error::ErrorCode::InvalidRequest,
                            "Selected Agent is not deployed on the requested Worker",
                        )
                    } else {
                        error
                    }
                })?;
            return Ok(node_id.to_owned());
        }
        let mut nodes = self.registry.nodes_for_agent(agent_id).await;
        if preferred_node_id.is_none() {
            nodes.sort_by_key(|node_id| node_id == EMBEDDED_NODE_ID);
        }
        let now = chrono::Utc::now().timestamp();
        for node_id in nodes {
            if preferred_node_id.is_some_and(|preferred| preferred != node_id) {
                continue;
            }
            let Some(node) = self.node_store.get(&node_id).await? else {
                continue;
            };
            let eligible = match node.mode {
                NodeMode::Embedded => {
                    node.node_id == EMBEDDED_NODE_ID && node.owner_user_id.is_none()
                }
                NodeMode::Remote => {
                    node.owner_user_id.as_deref() == Some(user_id)
                        && (node.credential_hash.is_none()
                            || (node.lease_expires_at.is_some_and(|expires| expires > now)
                                && node_has_recent_heartbeat(&node, now)))
                }
            };
            if eligible && node.status == NodeStatus::Enabled {
                return Ok(node_id);
            }
        }
        Err(TalosError::new(
            talos_error::ErrorCode::ResourceNotFound,
            "No eligible Agent execution node is registered",
        ))
    }

    async fn start(&self, params: AgentRunParams) -> TalosResult<AgentRunExecution> {
        let harness = self.context_harness(&params.context).await?;
        harness.start(params).await
    }

    async fn submit_input(&self, params: AgentInputParams) -> TalosResult<AgentAdmission> {
        let harness = self
            .harness(&params.agent_id, &params.identity.session_id, None)
            .await?;
        harness.submit_input(params).await
    }

    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome> {
        let harness = self
            .harness(&params.agent_id, &params.identity.session_id, None)
            .await?;
        harness.interrupt(params).await
    }

    async fn compact_session(&self, context: AgentContext) -> TalosResult<AgentRunExecution> {
        let harness = self.context_harness(&context).await?;
        harness.compact_session(context).await
    }

    async fn clear_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        let harness = self
            .harness(
                &params.agent_id,
                &params.session_id,
                Some(&params.workspace.user_id),
            )
            .await?;
        harness.clear_session(params, expected_history_cursor).await
    }

    async fn close_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        let harness = self
            .harness(
                &params.agent_id,
                &params.session_id,
                Some(&params.workspace.user_id),
            )
            .await?;
        harness.close_session(params, expected_history_cursor).await
    }

    async fn shutdown(&self, timeout: Duration) -> TalosResult<()> {
        let deadline = Instant::now() + timeout;
        for harness in self.registry.owned_harnesses().await {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| {
                    TalosError::new(
                        talos_error::ErrorCode::Timeout,
                        "Agent Coordinator shutdown timed out",
                    )
                })?;
            harness.shutdown(remaining).await?;
        }
        Ok(())
    }
}

fn node_has_recent_heartbeat(node: &talos_store::node::NodeRecord, now: i64) -> bool {
    node.health == NodeHealth::Healthy
        && agent_protocol::heartbeat_is_recent(node.last_heartbeat_at, now)
}

#[cfg(test)]
mod affinity_tests {
    use std::path::Path;

    use talos_storage::SqliteStorage;
    use talos_store::node::{
        CreateNodeParams, DatabaseAgentNodeStore, HeartbeatNodeParams, LeaseCheck,
        RegisterNodeParams,
    };
    use talos_store::session::{DatabaseSessionStore, MockSessionStore};

    use super::*;
    use crate::test_support::TestHarness;

    #[tokio::test]
    async fn pinned_session_does_not_fall_back_when_its_remote_node_disappears() {
        let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
        talos_store::migration::run_migrations(
            storage.pool(),
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
        )
        .await
        .unwrap();
        let node_store = Arc::new(DatabaseAgentNodeStore::new(storage.pool().clone()));
        let now = chrono::Utc::now().timestamp();
        node_store
            .upsert_embedded(NodeStatus::Enabled, now)
            .await
            .unwrap();
        node_store
            .create(CreateNodeParams {
                node_id: "z-laptop".into(),
                owner_user_id: Some("user-1".into()),
                display_name: "Laptop".into(),
                mode: NodeMode::Remote,
                endpoint: Some("https://127.0.0.1:18801".into()),
                credential_hash: Some([1; 32]),
                credential_expires_at: Some(now + 3600),
                now_at: now,
            })
            .await
            .unwrap();
        node_store
            .register(RegisterNodeParams {
                system_info: None,
                node_id: "z-laptop".into(),
                expected_credential_hash: [1; 32],
                lease_token_hash: [2; 32],
                lease_expires_at: now + 90,
                now_at: now,
            })
            .await
            .unwrap();
        node_store
            .heartbeat(HeartbeatNodeParams {
                lease: LeaseCheck {
                    node_id: "z-laptop".into(),
                    token_hash: [2; 32],
                    now_at: now,
                },
                health: NodeHealth::Healthy,
            })
            .await
            .unwrap();
        let store = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
        let mut record = MockSessionStore::new()
            .get_session_record("session-1")
            .await
            .unwrap()
            .unwrap();
        record.user_id = "user-1".into();
        record.execution_node_id = "z-laptop".into();
        store.insert_session_record(&record).await.unwrap();

        let coordinator = CoordinatorImpl::new(
            Arc::new(talos_router::RouterImpl::new()),
            store,
            node_store.clone(),
        );
        let embedded: Arc<dyn AgentHarness> = Arc::new(TestHarness);
        let remote: Arc<dyn AgentHarness> = Arc::new(TestHarness);
        coordinator
            .register_agent("embedded".into(), "default".into(), embedded)
            .await
            .unwrap();
        coordinator
            .register_agent("z-laptop".into(), "default".into(), remote.clone())
            .await
            .unwrap();
        assert_eq!(
            coordinator
                .select_execution_node("default", "user-1", None)
                .await
                .unwrap(),
            "z-laptop"
        );
        assert_eq!(
            coordinator
                .select_execution_node("default", "user-2", None)
                .await
                .unwrap(),
            "embedded"
        );
        assert!(coordinator
            .select_execution_node("default", "user-2", Some("z-laptop"))
            .await
            .is_err());
        coordinator
            .register_agent(
                "embedded".into(),
                "only-embedded".into(),
                Arc::new(TestHarness),
            )
            .await
            .unwrap();
        assert!(coordinator
            .select_execution_node("only-embedded", "user-1", Some("z-laptop"))
            .await
            .unwrap_err()
            .is_code(talos_error::ErrorCode::InvalidRequest));
        assert!(Arc::ptr_eq(
            &coordinator
                .harness("default", "session-1", Some("user-1"))
                .await
                .unwrap(),
            &remote
        ));
        node_store
            .heartbeat(HeartbeatNodeParams {
                lease: LeaseCheck {
                    node_id: "z-laptop".into(),
                    token_hash: [2; 32],
                    now_at: now - HEARTBEAT_TIMEOUT_SECONDS - 1,
                },
                health: NodeHealth::Healthy,
            })
            .await
            .unwrap();
        assert_eq!(
            coordinator
                .select_execution_node("default", "user-1", None)
                .await
                .unwrap(),
            "embedded"
        );
        assert!(coordinator
            .harness("default", "session-1", Some("user-1"))
            .await
            .err()
            .unwrap()
            .is_code(talos_error::ErrorCode::ServiceUnavailable));
        node_store
            .heartbeat(HeartbeatNodeParams {
                lease: LeaseCheck {
                    node_id: "z-laptop".into(),
                    token_hash: [2; 32],
                    now_at: chrono::Utc::now().timestamp(),
                },
                health: NodeHealth::Healthy,
            })
            .await
            .unwrap();
        coordinator.unregister_remote_node("z-laptop").await;
        let error = coordinator
            .harness("default", "session-1", Some("user-1"))
            .await
            .err()
            .unwrap();
        assert!(error.is_code(talos_error::ErrorCode::ServiceUnavailable));
    }
}
