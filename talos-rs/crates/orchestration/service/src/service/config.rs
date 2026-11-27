//! Product policy is local to its owner; node configuration is routed to Worker.
use super::TalosService;
use crate::Actor;
use talos_error::TalosResult;
use talos_types::*;
impl TalosService {
    /// Selects the owning product's configuration backend independently of its Workers.
    #[must_use]
    pub fn with_product_config_store(
        mut self,
        store: std::sync::Arc<dyn talos_config::ConfigStore>,
    ) -> Self {
        self.product_config_store = store;
        self
    }

    pub async fn get_config(
        &self,
        actor: &Actor,
        mut request: ConfigGetRequest,
    ) -> TalosResult<ConfigResponse> {
        if request.path == ConfigPath::SessionMessageSendPolicy {
            if actor.user_id.trim().is_empty() {
                return Err(talos_error::TalosError::new(
                    talos_error::ErrorCode::Unauthenticated,
                    "Product configuration requires an authenticated actor.",
                ));
            }
            self.access.ensure_worker_access(actor, "embedded").await?;
            return self
                .product_config_store
                .load_session_message_send_policy()
                .map(|policy| ConfigResponse::SessionMessageSendPolicy { policy });
        }
        let node = request.node_id.take();
        self.worker_environment(actor, node.as_deref())
            .await?
            .get_config(crate::access::worker_environment_context(actor), request)
            .await
    }
    pub async fn update_config(
        &self,
        actor: &Actor,
        mut request: ConfigUpdateRequest,
    ) -> TalosResult<ConfigResponse> {
        if let ConfigValue::SessionMessageSendPolicy(policy) = &request.value {
            if actor.user_id.trim().is_empty() {
                return Err(talos_error::TalosError::new(
                    talos_error::ErrorCode::Unauthenticated,
                    "Product configuration requires an authenticated actor.",
                ));
            }
            self.access.ensure_worker_access(actor, "embedded").await?;
            self.product_config_store
                .save_session_message_send_policy(*policy)?;
            return Ok(ConfigResponse::SessionMessageSendPolicy { policy: *policy });
        }
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let node = self.resolve_computer_node(actor, &node).await?;
        let runtime_update = request.value.requires_runtime_reload();
        let _guard = if runtime_update {
            Some(self.execution_gate.for_node(&node).write_owned().await)
        } else {
            None
        };
        let environment = self.worker_environment(actor, Some(&node)).await?;
        if runtime_update {
            self.sessions.reject_nonterminal_turns(&node).await?;
        }
        environment
            .update_config(crate::access::worker_environment_context(actor), request)
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
            .delete_config(crate::access::worker_environment_context(actor), request)
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
            .reload_config(crate::access::worker_environment_context(actor), request)
            .await
    }
}
