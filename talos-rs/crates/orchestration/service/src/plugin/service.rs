use std::sync::Arc;
use talos_config::{PluginsRuntimeConfig, SkillsRuntimeConfig};
use talos_error::TalosResult;
use talos_plugin::PluginManager;
use talos_skills::SkillRoots;
use talos_types::{
    PluginDisableRequest, PluginDisableResponse, PluginEnableRequest, PluginEnableResponse,
    PluginInstallRequest, PluginInstallResponse, PluginListRequest, PluginListResponse,
    PluginReadRequest, PluginReadResponse, PluginSearchRequest, PluginSearchResponse,
    PluginSourceAddRequest, PluginSourceAddResponse, PluginSourceListRequest,
    PluginSourceListResponse, PluginSourceRefreshRequest, PluginSourceRefreshResponse,
    PluginSourceRemoveRequest, PluginSourceRemoveResponse, PluginUninstallRequest,
    PluginUninstallResponse, PluginUpdateRequest, PluginUpdateResponse,
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct PluginManagerService {
    pub(super) manager: PluginManager,
    pub(crate) execution_gate: Arc<RwLock<()>>,
    pub(super) mcp: Option<McpContext>,
    pub(super) config_service: Option<crate::ConfigService>,
}

#[derive(Clone)]
pub(super) struct McpContext {
    pub runtime: Arc<talos_mcp::TalosMcpRuntime>,
    pub config: Arc<std::sync::RwLock<talos_config::Config>>,
    pub credentials: super::McpCredentialStore,
    pub oauth: Arc<super::oauth::OAuthAttempts>,
}

impl PluginManagerService {
    pub fn new(config: PluginsRuntimeConfig, skills: SkillsRuntimeConfig) -> Self {
        Self {
            manager: PluginManager::new(config).with_max_skill_md_bytes(skills.max_skill_md_bytes),
            execution_gate: Arc::default(),
            mcp: None,
            config_service: None,
        }
    }

    pub fn with_manager(manager: PluginManager) -> Self {
        Self {
            manager,
            execution_gate: Arc::default(),
            mcp: None,
            config_service: None,
        }
    }

    /// Attaches the Worker's existing atomic configuration publisher for configured MCP credentials.
    /// Hosts must compose this after the MCP hook; it never owns product persistence.
    #[must_use]
    pub fn with_config_service(mut self, service: crate::ConfigService) -> Self {
        self.config_service = Some(service);
        self
    }

    /// The node-local gate shared by actual execution and environment commits.
    pub fn execution_gate(&self) -> Arc<RwLock<()>> {
        self.execution_gate.clone()
    }

    /// Connects plugin publication and status to the application-owned MCP owner.
    pub fn with_mcp(
        mut self,
        runtime: Arc<talos_mcp::TalosMcpRuntime>,
        hook: talos_plugin::McpChangeHook,
        config: Arc<std::sync::RwLock<talos_config::Config>>,
        credentials: super::McpCredentialStore,
    ) -> Self {
        self.manager = self.manager.with_mcp_change_hook(hook);
        self.mcp = Some(McpContext {
            runtime,
            config,
            credentials,
            oauth: Arc::default(),
        });
        self
    }

    /// Serializes terminal connection shutdown with plugin/configuration commits.
    pub async fn shutdown_mcp(&self) -> TalosResult<()> {
        let _guard = self.execution_gate.write().await;
        match &self.mcp {
            Some(context) => {
                let finishing = context.oauth.stop();
                let result = context.runtime.shutdown().await;
                drop(_guard);
                for finished in finishing {
                    finished.cancelled().await;
                }
                context.oauth.tasks.wait().await;
                result
            }
            None => Ok(()),
        }
    }

    pub fn list_mcp_servers(
        &self,
        request: talos_types::McpServerListRequest,
    ) -> TalosResult<talos_types::McpServerListResponse> {
        self.mcp
            .as_ref()
            .ok_or_else(|| {
                talos_error::TalosError::new(
                    talos_error::ErrorCode::ServiceUnavailable,
                    "MCP management is not attached to this application.",
                )
            })?
            .runtime
            .list_servers(request)
    }

    pub fn list_sources(
        &self,
        request: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse> {
        self.manager.list_sources(request)
    }

    pub fn add_source(
        &self,
        request: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse> {
        self.manager.add_source(request).map(|mut response| {
            response
                .plugins
                .iter_mut()
                .for_each(super::redaction::plugin);
            response
        })
    }

    pub fn refresh_source(
        &self,
        request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        self.manager.refresh_source(request).map(|mut response| {
            response
                .plugins
                .iter_mut()
                .for_each(super::redaction::plugin);
            response
        })
    }

    pub fn remove_source(
        &self,
        request: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse> {
        self.manager.remove_source(request)
    }

    pub fn search(&self, request: PluginSearchRequest) -> TalosResult<PluginSearchResponse> {
        self.manager.search(request).map(|mut response| {
            response
                .plugins
                .iter_mut()
                .for_each(super::redaction::plugin);
            response
        })
    }

    pub fn list_plugins(&self, request: PluginListRequest) -> TalosResult<PluginListResponse> {
        self.manager.list_plugins(request).map(|mut response| {
            response
                .plugins
                .iter_mut()
                .for_each(super::redaction::plugin);
            response
        })
    }

    pub fn read(&self, request: PluginReadRequest) -> TalosResult<PluginReadResponse> {
        super::redaction::read(self.manager.read(request)?)
    }

    pub async fn install(
        &self,
        request: PluginInstallRequest,
    ) -> TalosResult<PluginInstallResponse> {
        let _guard = self.commit_guard()?;
        let result = self.install_with_credentials(request).map(|mut response| {
            super::redaction::plugin(&mut response.plugin);
            response
        });
        self.recover_credentials();
        result
    }

    pub async fn update(&self, request: PluginUpdateRequest) -> TalosResult<PluginUpdateResponse> {
        let _guard = self.commit_guard()?;
        let result = self.manager.update(request).map(|mut response| {
            super::redaction::plugin(&mut response.plugin);
            response
        });
        self.recover_credentials();
        result
    }

    pub async fn uninstall(
        &self,
        request: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse> {
        let _guard = self.commit_guard()?;
        let result = self.manager.uninstall(request);
        self.recover_credentials();
        result
    }

    pub async fn enable(&self, request: PluginEnableRequest) -> TalosResult<PluginEnableResponse> {
        let _guard = self.commit_guard()?;
        self.manager.enable(request).map(|mut response| {
            super::redaction::plugin(&mut response.plugin);
            response
        })
    }

    pub async fn disable(
        &self,
        request: PluginDisableRequest,
    ) -> TalosResult<PluginDisableResponse> {
        let _guard = self.commit_guard()?;
        let result = self.manager.disable(request).map(|mut response| {
            super::redaction::plugin(&mut response.plugin);
            response
        });
        self.recover_credentials();
        result
    }

    pub(super) fn commit_guard(&self) -> TalosResult<tokio::sync::RwLockWriteGuard<'_, ()>> {
        self.execution_gate.try_write().map_err(|_| {
            talos_error::TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Worker environment cannot change while execution is active",
            )
        })
    }

    pub fn skill_roots(&self, config: &SkillsRuntimeConfig) -> TalosResult<SkillRoots> {
        self.manager.skill_roots(config)
    }

    pub fn mcp_servers(&self) -> TalosResult<talos_plugin::McpDiscovery> {
        self.manager.mcp_servers()
    }
}
