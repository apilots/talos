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
    pub(super) browser: Option<Arc<talos_browser::BrowserRuntime>>,
    pub(super) preparation_slots: Arc<tokio::sync::Semaphore>,
    pub(super) preparations: Arc<
        std::sync::Mutex<
            std::collections::BTreeMap<String, super::preparation::PendingPluginChange>,
        >,
    >,
}

#[derive(Clone)]
pub(super) struct McpContext {
    pub runtime: Arc<talos_mcp::TalosMcpRuntime>,
    pub credentials: super::McpCredentialStore,
    pub oauth: Arc<super::oauth::OAuthAttempts>,
}

impl PluginManagerService {
    pub fn new(config: PluginsRuntimeConfig, skills: SkillsRuntimeConfig) -> Self {
        Self {
            manager: PluginManager::new(config).with_max_skill_md_bytes(skills.max_skill_md_bytes),
            execution_gate: Arc::default(),
            mcp: None,
            browser: None,
            preparations: Arc::default(),
            preparation_slots: Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }

    pub fn with_manager(manager: PluginManager) -> Self {
        Self {
            manager,
            execution_gate: Arc::default(),
            mcp: None,
            browser: None,
            preparations: Arc::default(),
            preparation_slots: Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }

    #[must_use]
    pub fn with_browser(mut self, browser: Arc<talos_browser::BrowserRuntime>) -> Self {
        self.browser = Some(browser);
        self
    }

    /// Startup composition uses the installed Plugin revision, never a hardcoded library version.
    pub async fn initialize_browser_environment(&self) -> TalosResult<()> {
        if let (Some(browser), Some((installation, lock))) = (
            &self.browser,
            self.manager.installed_browser_requirements()?,
        ) {
            if let Some(prepared) = browser.reuse_browser_environment(&lock).await? {
                self.activate_installed_browser(installation, prepared)
                    .await?;
            }
        }
        Ok(())
    }

    /// Bounded startup repair uses the same source refresh and Plugin install owners.
    pub async fn bootstrap_official_browser(&self) -> TalosResult<()> {
        if self.manager.installed_browser_requirements()?.is_none() {
            let manager = self.manager.clone();
            tokio::task::spawn_blocking(move || {
                let source = talos_config::official_mcp_source();
                let source_id = manager.list_sources(PluginSourceListRequest { node_id: None })?
                    .sources.into_iter().find(|record| {
                        matches!(&record.location, talos_types::MarketplaceSourceLocation::Git {url, reference}
                            if *url == source.url && *reference == source.reference)
                            && record.sparse_paths == source.sparse_paths
                    }).ok_or_else(|| talos_error::TalosError::internal("Official MCP source is unavailable."))?.id;
                manager.refresh_source(PluginSourceRefreshRequest { node_id: None, source_id })
            }).await.map_err(|error| {
                talos_error::TalosError::internal_with_source("Official MCP discovery could not finish.", error)
            })??;
            if let Some(request) = self.manager.default_browser_install_request()? {
                self.install(request).await?;
            }
        }
        if let (Some(browser), Some((installation, lock))) = (
            &self.browser,
            self.manager.installed_browser_requirements()?,
        ) {
            if let Some(prepared) = browser.prepare_browser_environment(&lock).await? {
                self.activate_installed_browser(installation, prepared)
                    .await?;
            }
        }
        Ok(())
    }

    async fn activate_installed_browser(
        &self,
        installation: talos_types::PluginInstall,
        prepared: talos_browser::PreparedBrowserEnvironment,
    ) -> TalosResult<()> {
        let _guard = self.commit_guard()?;
        if self
            .manager
            .installed_browser_requirements()?
            .map(|(current, _)| current)
            != Some(installation)
        {
            return Err(talos_error::TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Browser changed during environment preparation.",
            ));
        }
        self.activate_prepared_browser(Some(prepared)).await
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
        credentials: super::McpCredentialStore,
    ) -> Self {
        self.manager = self.manager.with_mcp_change_hook(hook);
        self.mcp = Some(McpContext {
            runtime,
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
        let prepared = self
            .prepare_change(agent_protocol::WorkerPluginChange::Install(request.clone()))
            .await?;
        self.install_prepared(request, prepared).await
    }

    pub(super) async fn install_prepared(
        &self,
        mut request: PluginInstallRequest,
        prepared: super::preparation::PreparedServiceChange,
    ) -> TalosResult<PluginInstallResponse> {
        let _guard = self.commit_guard()?;
        self.prepare_install_credentials(&mut request)?;
        let result = self
            .manager
            .install_prepared(request, prepared.plugin)
            .map(|mut response| {
                super::redaction::plugin(&mut response.plugin);
                response
            });
        self.recover_credentials();
        if result.is_ok() {
            self.activate_prepared_browser(prepared.environment).await?;
        }
        result
    }

    pub async fn update(&self, request: PluginUpdateRequest) -> TalosResult<PluginUpdateResponse> {
        let prepared = self
            .prepare_change(agent_protocol::WorkerPluginChange::Update(request.clone()))
            .await?;
        self.update_prepared(request, prepared).await
    }

    pub(super) async fn update_prepared(
        &self,
        request: PluginUpdateRequest,
        prepared: super::preparation::PreparedServiceChange,
    ) -> TalosResult<PluginUpdateResponse> {
        let _guard = self.commit_guard()?;
        let result = self
            .manager
            .update_prepared(request, prepared.plugin)
            .map(|mut response| {
                super::redaction::plugin(&mut response.plugin);
                response
            });
        self.recover_credentials();
        if result.is_ok() {
            self.activate_prepared_browser(prepared.environment).await?;
        }
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
        let mut response = self.manager.enable(request)?;
        if response.plugin.id.0 == talos_types::OFFICIAL_BROWSER_PLUGIN_ID {
            if let Some(browser) = &self.browser {
                browser.plugin_enabled().await?;
            }
        }
        super::redaction::plugin(&mut response.plugin);
        Ok(response)
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
        let response = result?;
        if response.plugin.id.0 == talos_types::OFFICIAL_BROWSER_PLUGIN_ID {
            if let Some(browser) = &self.browser {
                browser.plugin_disabled().await?;
            }
        }
        Ok(response)
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
