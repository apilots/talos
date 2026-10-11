//! Product authorization and node admission; configuration files belong to Worker.
use super::TalosService;
use crate::Actor;
use agent_protocol::WorkerEnvironment;
use std::sync::Arc;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;
impl TalosService {
    pub(crate) async fn managed_environment(
        &self,
        actor: &Actor,
        node_id: Option<&str>,
    ) -> TalosResult<Arc<dyn WorkerEnvironment>> {
        if node_id.unwrap_or("embedded") == "embedded"
            && !actor.auth_disabled
            && self.mode() != talos_config::DeploymentMode::Local
            && actor.role != talos_auth::UserRole::Admin
        {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Shared Computer environment management requires an admin user",
            ));
        }
        self.worker_environment(actor, node_id).await
    }
    pub async fn get_config(
        &self,
        actor: &Actor,
        mut request: ConfigGetRequest,
    ) -> TalosResult<ConfigResponse> {
        let node = request.node_id.take();
        self.managed_environment(actor, node.as_deref())
            .await?
            .get_config(actor.worker_environment_context(), request)
            .await
    }
    pub async fn update_config(
        &self,
        actor: &Actor,
        mut request: ConfigUpdateRequest,
    ) -> TalosResult<ConfigResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.managed_environment(actor, Some(&node)).await?;
        self.sessions.reject_nonterminal_turns(&node).await?;
        environment
            .update_config(actor.worker_environment_context(), request)
            .await
    }
    pub async fn delete_config(
        &self,
        actor: &Actor,
        mut request: ConfigDeleteRequest,
    ) -> TalosResult<ConfigDeleteResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.managed_environment(actor, Some(&node)).await?;
        self.sessions.reject_nonterminal_turns(&node).await?;
        environment
            .delete_config(actor.worker_environment_context(), request)
            .await
    }
    pub async fn reload_config(
        &self,
        actor: &Actor,
        mut request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.managed_environment(actor, Some(&node)).await?;
        self.sessions.reject_nonterminal_turns(&node).await?;
        environment
            .reload_config(actor.worker_environment_context(), request)
            .await
    }
}
