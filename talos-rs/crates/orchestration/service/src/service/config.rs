//! Product authorization and node admission; configuration files belong to Worker.
use super::TalosService;
use crate::Actor;
use talos_error::TalosResult;
use talos_types::*;
impl TalosService {
    pub async fn get_config(
        &self,
        actor: &Actor,
        mut request: ConfigGetRequest,
    ) -> TalosResult<ConfigResponse> {
        let node = request.node_id.take();
        self.worker_environment(actor, node.as_deref())
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
        let node = self.resolve_computer_node(actor, &node).await?;
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.worker_environment(actor, Some(&node)).await?;
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
        let node = self.resolve_computer_node(actor, &node).await?;
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.worker_environment(actor, Some(&node)).await?;
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
        let node = self.resolve_computer_node(actor, &node).await?;
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.worker_environment(actor, Some(&node)).await?;
        self.sessions.reject_nonterminal_turns(&node).await?;
        environment
            .reload_config(actor.worker_environment_context(), request)
            .await
    }
}
