//! Actor-authorized target routing; all installation and credential I/O belongs to Worker.
use super::TalosService;
use crate::Actor;
use talos_error::TalosResult;
use talos_types::*;
impl TalosService {
    pub async fn list_plugin_sources(
        &self,
        actor: &Actor,
        mut request: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let environment = self.worker_environment(actor, Some(&node)).await?;
        environment
            .list_plugin_sources(actor.worker_environment_context(), request)
            .await
    }
    pub async fn add_plugin_source(
        &self,
        actor: &Actor,
        mut request: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let node = self.resolve_computer_node(actor, &node).await?;
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.worker_environment(actor, Some(&node)).await?;
        self.sessions.reject_nonterminal_turns(&node).await?;
        environment
            .add_plugin_source(actor.worker_environment_context(), request)
            .await
    }
    pub async fn refresh_plugin_source(
        &self,
        actor: &Actor,
        mut request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let node = self.resolve_computer_node(actor, &node).await?;
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.worker_environment(actor, Some(&node)).await?;
        self.sessions.reject_nonterminal_turns(&node).await?;
        environment
            .refresh_plugin_source(actor.worker_environment_context(), request)
            .await
    }
    pub async fn remove_plugin_source(
        &self,
        actor: &Actor,
        mut request: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let node = self.resolve_computer_node(actor, &node).await?;
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.worker_environment(actor, Some(&node)).await?;
        self.sessions.reject_nonterminal_turns(&node).await?;
        environment
            .remove_plugin_source(actor.worker_environment_context(), request)
            .await
    }
    pub async fn search_plugins(
        &self,
        actor: &Actor,
        mut request: PluginSearchRequest,
    ) -> TalosResult<PluginSearchResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let environment = self.worker_environment(actor, Some(&node)).await?;
        environment
            .search_plugins(actor.worker_environment_context(), request)
            .await
    }
    pub async fn list_plugins(
        &self,
        actor: &Actor,
        mut request: PluginListRequest,
    ) -> TalosResult<PluginListResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let environment = self.worker_environment(actor, Some(&node)).await?;
        environment
            .list_plugins(actor.worker_environment_context(), request)
            .await
    }
    pub async fn read_plugin(
        &self,
        actor: &Actor,
        mut request: PluginReadRequest,
    ) -> TalosResult<PluginReadResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let environment = self.worker_environment(actor, Some(&node)).await?;
        environment
            .read_plugin(actor.worker_environment_context(), request)
            .await
    }
    pub async fn install_plugin(
        &self,
        actor: &Actor,
        mut request: PluginInstallRequest,
    ) -> TalosResult<PluginInstallResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let node = self.resolve_computer_node(actor, &node).await?;
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.worker_environment(actor, Some(&node)).await?;
        self.sessions.reject_nonterminal_turns(&node).await?;
        environment
            .install_plugin(actor.worker_environment_context(), request)
            .await
    }
    pub async fn update_plugin(
        &self,
        actor: &Actor,
        mut request: PluginUpdateRequest,
    ) -> TalosResult<PluginUpdateResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let node = self.resolve_computer_node(actor, &node).await?;
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.worker_environment(actor, Some(&node)).await?;
        self.sessions.reject_nonterminal_turns(&node).await?;
        environment
            .update_plugin(actor.worker_environment_context(), request)
            .await
    }
    pub async fn uninstall_plugin(
        &self,
        actor: &Actor,
        mut request: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let node = self.resolve_computer_node(actor, &node).await?;
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.worker_environment(actor, Some(&node)).await?;
        self.sessions.reject_nonterminal_turns(&node).await?;
        environment
            .uninstall_plugin(actor.worker_environment_context(), request)
            .await
    }
    pub async fn enable_plugin(
        &self,
        actor: &Actor,
        mut request: PluginEnableRequest,
    ) -> TalosResult<PluginEnableResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let node = self.resolve_computer_node(actor, &node).await?;
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.worker_environment(actor, Some(&node)).await?;
        self.sessions.reject_nonterminal_turns(&node).await?;
        environment
            .enable_plugin(actor.worker_environment_context(), request)
            .await
    }
    pub async fn disable_plugin(
        &self,
        actor: &Actor,
        mut request: PluginDisableRequest,
    ) -> TalosResult<PluginDisableResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let node = self.resolve_computer_node(actor, &node).await?;
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.worker_environment(actor, Some(&node)).await?;
        self.sessions.reject_nonterminal_turns(&node).await?;
        environment
            .disable_plugin(actor.worker_environment_context(), request)
            .await
    }
    pub async fn browser(
        &self,
        actor: &Actor,
        mut request: BrowserRequest,
    ) -> TalosResult<BrowserSnapshot> {
        if matches!(request.operation, BrowserOperation::OpenLocal) {
            return Err(TalosError::localized(
                talos_error::ErrorLocalization::BrowserCapabilityUnavailable {
                    operation: talos_error::BrowserCapabilityOperation::OpenLocal,
                    reason: talos_error::BrowserCapabilityReason::ClientMismatch,
                },
            ));
        }
        let node = request.node_id.take().ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Select a Computer for Browser settings.",
            )
        })?;
        let environment = self.worker_environment(actor, Some(&node)).await?;
        if matches!(
            request.operation,
            BrowserOperation::TakeControl { .. } | BrowserOperation::ReleaseControl { .. }
        ) {
            let snapshot = environment
                .browser(
                    actor.worker_environment_context(),
                    BrowserRequest {
                        node_id: None,
                        operation: BrowserOperation::Read,
                    },
                )
                .await?;
            if !matches!(
                snapshot.display,
                BrowserDisplayCapability::DockerDesktop { .. }
            ) {
                return Err(TalosError::localized(
                    talos_error::ErrorLocalization::BrowserCapabilityUnavailable {
                        operation: if matches!(
                            request.operation,
                            BrowserOperation::TakeControl { .. }
                        ) {
                            talos_error::BrowserCapabilityOperation::TakeControl
                        } else {
                            talos_error::BrowserCapabilityOperation::ReleaseControl
                        },
                        reason: if matches!(
                            snapshot.host,
                            BrowserHostKind::Desktop | BrowserHostKind::Native
                        ) {
                            talos_error::BrowserCapabilityReason::NativeHost
                        } else {
                            talos_error::BrowserCapabilityReason::NotBundled
                        },
                    },
                ));
            }
        }
        environment
            .browser(actor.worker_environment_context(), request)
            .await
    }

    pub async fn list_mcp_servers(
        &self,
        actor: &Actor,
        mut request: McpServerListRequest,
    ) -> TalosResult<McpServerListResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let environment = self.worker_environment(actor, Some(&node)).await?;
        environment
            .list_mcp_servers(actor.worker_environment_context(), request)
            .await
    }
    pub async fn read_mcp_authentication(
        &self,
        actor: &Actor,
        mut request: McpAuthenticationReadRequest,
    ) -> TalosResult<McpAuthenticationResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let environment = self.worker_environment(actor, Some(&node)).await?;
        environment
            .read_mcp_authentication(actor.worker_environment_context(), request)
            .await
    }
    pub async fn update_mcp_credentials(
        &self,
        actor: &Actor,
        mut request: McpCredentialsUpdateRequest,
    ) -> TalosResult<McpAuthenticationResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let node = self.resolve_computer_node(actor, &node).await?;
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.worker_environment(actor, Some(&node)).await?;
        self.sessions.reject_nonterminal_turns(&node).await?;
        environment
            .update_mcp_credentials(actor.worker_environment_context(), request)
            .await
    }
    pub async fn import_mcp_server(
        &self,
        actor: &Actor,
        mut request: McpServerImportRequest,
    ) -> TalosResult<PluginInstallResponse> {
        let node = request
            .node_id
            .take()
            .unwrap_or_else(|| "embedded".to_owned());
        let node = self.resolve_computer_node(actor, &node).await?;
        let _guard = self.execution_gate.for_node(&node).write_owned().await;
        let environment = self.worker_environment(actor, Some(&node)).await?;
        self.sessions.reject_nonterminal_turns(&node).await?;
        environment
            .import_mcp_server(actor.worker_environment_context(), request)
            .await
    }
}
