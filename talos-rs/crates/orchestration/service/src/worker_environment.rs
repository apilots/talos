//! Reusable environment implementation assembled and owned by TalosWorker.
use crate::{fs::FsService, Actor};
use agent_protocol::{
    AgentFuture, WorkerEnvironment, WorkerEnvironmentContext, WorkerPluginChange,
    WorkerPluginInstall, WorkerPluginPrepared, WorkerPluginUpdate,
};
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};
use talos_config::Config;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;

pub struct WorkerEnvironmentService {
    config: Arc<RwLock<Config>>,
    preview_roots: crate::fs::preview::PreviewRoots,
    home: Option<PathBuf>,
    workspace_memory: Arc<crate::UserWorkspaceMemoryService>,
    memory_service: Option<crate::MemoryService>,
    plugins: Option<crate::PluginManagerService>,
    config_service: Option<crate::ConfigService>,
    browser: Option<Arc<talos_browser::BrowserRuntime>>,
}

impl WorkerEnvironmentService {
    /// The Worker passes its live configuration and host home at composition.
    pub fn new(config: Arc<RwLock<Config>>, home: Option<PathBuf>) -> Self {
        Self {
            config,
            preview_roots: Default::default(),
            home,
            workspace_memory: Arc::new(crate::UserWorkspaceMemoryService::new()),
            memory_service: None,
            plugins: None,
            config_service: None,
            browser: None,
        }
    }
    #[must_use]
    pub fn with_user_workspace_memory(
        mut self,
        memory: Arc<crate::UserWorkspaceMemoryService>,
    ) -> Self {
        self.workspace_memory = memory;
        self
    }
    #[must_use]
    pub fn with_memory_service(mut self, memory: crate::MemoryService) -> Self {
        self.memory_service = Some(memory);
        self
    }
    #[must_use]
    pub fn with_plugin_manager(mut self, plugins: crate::PluginManagerService) -> Self {
        self.plugins = Some(plugins);
        self
    }
    pub fn with_config_service(mut self, service: crate::ConfigService) -> Self {
        self.config_service = Some(service);
        self
    }

    pub fn with_browser(mut self, browser: Arc<talos_browser::BrowserRuntime>) -> Self {
        self.browser = Some(browser);
        self
    }

    fn plugin_manager(&self) -> TalosResult<&crate::PluginManagerService> {
        self.plugins.as_ref().ok_or_else(|| {
            TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Worker plugin management is unavailable",
            )
        })
    }
    fn memory(&self, context: &WorkerEnvironmentContext) -> TalosResult<crate::MemoryService> {
        self.memory_service
            .as_ref()
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Worker Memory service is unavailable",
                )
            })?
            .with_config(self.workspace_config(context)?)
    }
    fn skills(&self, context: &WorkerEnvironmentContext) -> TalosResult<crate::SkillsService> {
        Self::actor(context)?;
        let plugins = self.plugins.as_ref().ok_or_else(|| {
            TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Worker Skills service is unavailable",
            )
        })?;
        Ok(crate::SkillsService::new(
            self.snapshot().agent.kernel.skills,
            plugins.clone(),
        ))
    }
    fn workspace_config(&self, context: &WorkerEnvironmentContext) -> TalosResult<Config> {
        Self::actor(context)?;
        let mut config = self.snapshot();
        if let Some(id) = &context.workspace_id {
            config.resolve_workspace(id, &context.user_id)?;
            config.workspaces.default_workspace_id = id.clone();
        }
        Ok(config)
    }
    fn docs(&self, context: &WorkerEnvironmentContext) -> TalosResult<crate::WorkspaceDocService> {
        Ok(
            crate::WorkspaceDocService::new(self.workspace_config(context)?)
                .with_user_workspace_memory(self.workspace_memory.clone()),
        )
    }
    fn snapshot(&self) -> Config {
        self.config
            .read()
            .expect("Worker config lock poisoned")
            .clone()
    }
    fn fs(&self) -> FsService {
        FsService::new(self.snapshot().fs.directory_picker, self.home.clone())
    }
    fn actor(context: &WorkerEnvironmentContext) -> TalosResult<Actor> {
        if context.user_id.trim().is_empty() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Worker actor cannot be empty",
            ));
        }
        Ok(Actor::regular(&context.user_id).with_username(context.username.clone()))
    }
}

macro_rules! plugin_environment_method {
    ($method:ident($params:ty) -> $result:ty => sync $owner:ident) => {
        fn $method(
            &self,
            context: WorkerEnvironmentContext,
            params: $params,
        ) -> AgentFuture<'_, TalosResult<$result>> {
            Box::pin(async move {
                Self::actor(&context)?;
                self.plugin_manager()?.$owner(params)
            })
        }
    };
    ($method:ident($params:ty) -> $result:ty => async $owner:ident) => {
        fn $method(
            &self,
            context: WorkerEnvironmentContext,
            params: $params,
        ) -> AgentFuture<'_, TalosResult<$result>> {
            Box::pin(async move {
                Self::actor(&context)?;
                self.plugin_manager()?.$owner(params).await
            })
        }
    };
}
impl WorkerEnvironment for WorkerEnvironmentService {
    fn desktop_open(
        &self,
        context: WorkerEnvironmentContext,
        params: WorkerDesktopOpen,
    ) -> AgentFuture<'_, TalosResult<()>> {
        Box::pin(async move {
            Self::actor(&context)?;
            self.browser
                .as_ref()
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Browser is unavailable on this Computer.",
                    )
                })?
                .open_desktop(context.user_id, params)
                .await
        })
    }

    fn browser(
        &self,
        context: WorkerEnvironmentContext,
        params: BrowserRequest,
    ) -> AgentFuture<'_, TalosResult<BrowserSnapshot>> {
        Box::pin(async move {
            Self::actor(&context)?;
            self.browser
                .as_ref()
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Browser is unavailable on this Computer.",
                    )
                })?
                .operate(context.user_id.clone(), params.operation)
                .await
        })
    }
    plugin_environment_method!(list_plugin_sources(PluginSourceListRequest) -> PluginSourceListResponse => sync list_sources);
    plugin_environment_method!(add_plugin_source(PluginSourceAddRequest) -> PluginSourceAddResponse => sync add_source);
    plugin_environment_method!(refresh_plugin_source(PluginSourceRefreshRequest) -> PluginSourceRefreshResponse => sync refresh_source);
    plugin_environment_method!(remove_plugin_source(PluginSourceRemoveRequest) -> PluginSourceRemoveResponse => sync remove_source);
    plugin_environment_method!(search_plugins(PluginSearchRequest) -> PluginSearchResponse => sync search);
    plugin_environment_method!(list_plugins(PluginListRequest) -> PluginListResponse => sync list_plugins);
    plugin_environment_method!(read_plugin(PluginReadRequest) -> PluginReadResponse => sync read);
    fn prepare_plugin_change(
        &self,
        context: WorkerEnvironmentContext,
        params: WorkerPluginChange,
    ) -> AgentFuture<'_, TalosResult<WorkerPluginPrepared>> {
        Box::pin(async move {
            Self::actor(&context)?;
            self.plugin_manager()?
                .prepare_plugin_change(context, params)
                .await
        })
    }
    fn cancel_plugin_change(
        &self,
        context: WorkerEnvironmentContext,
        params: WorkerPluginPrepared,
    ) -> AgentFuture<'_, TalosResult<()>> {
        Box::pin(async move {
            Self::actor(&context)?;
            self.plugin_manager()?.cancel_plugin_change(context, params)
        })
    }
    fn install_plugin(
        &self,
        context: WorkerEnvironmentContext,
        params: WorkerPluginInstall,
    ) -> AgentFuture<'_, TalosResult<PluginInstallResponse>> {
        Box::pin(async move {
            Self::actor(&context)?;
            self.plugin_manager()?
                .commit_plugin_install(context, params)
                .await
        })
    }
    fn update_plugin(
        &self,
        context: WorkerEnvironmentContext,
        params: WorkerPluginUpdate,
    ) -> AgentFuture<'_, TalosResult<PluginUpdateResponse>> {
        Box::pin(async move {
            Self::actor(&context)?;
            self.plugin_manager()?
                .commit_plugin_update(context, params)
                .await
        })
    }
    plugin_environment_method!(uninstall_plugin(PluginUninstallRequest) -> PluginUninstallResponse => async uninstall);
    plugin_environment_method!(enable_plugin(PluginEnableRequest) -> PluginEnableResponse => async enable);
    plugin_environment_method!(disable_plugin(PluginDisableRequest) -> PluginDisableResponse => async disable);
    plugin_environment_method!(list_mcp_servers(McpServerListRequest) -> McpServerListResponse => sync list_mcp_servers);
    plugin_environment_method!(read_mcp_authentication(McpAuthenticationReadRequest) -> McpAuthenticationResponse => sync read_mcp_authentication);
    plugin_environment_method!(update_mcp_credentials(McpCredentialsUpdateRequest) -> McpAuthenticationResponse => async update_mcp_credentials);
    plugin_environment_method!(import_mcp_server(McpServerImportRequest) -> PluginInstallResponse => async import_mcp_server);
    fn start_mcp_authentication(
        &self,
        context: WorkerEnvironmentContext,
        params: agent_protocol::McpAuthenticationStartParams,
    ) -> AgentFuture<'_, TalosResult<McpAuthenticationStartResponse>> {
        Box::pin(async move {
            Self::actor(&context)?;
            if let agent_protocol::McpOAuthCallbackTarget::ControlPlaneRelay { uri } =
                &params.callback_target
            {
                if uri != &crate::mcp_relay_callback_uri(&self.snapshot().cloud.agent_base_url)? {
                    return Err(TalosError::new(
                        ErrorCode::PermissionDenied,
                        "MCP callback origin is not the configured control plane.",
                    ));
                }
            }
            self.plugin_manager()?
                .start_mcp_authentication_with_callback(params, Some(context.user_id))
                .await
        })
    }
    fn complete_mcp_authentication(
        &self,
        context: WorkerEnvironmentContext,
        params: agent_protocol::McpAuthenticationCallbackParams,
    ) -> AgentFuture<'_, TalosResult<McpAuthenticationResponse>> {
        Box::pin(async move {
            Self::actor(&context)?;
            self.plugin_manager()?
                .complete_mcp_authentication(&context.user_id, params)
                .await
        })
    }

    fn get_config(
        &self,
        context: WorkerEnvironmentContext,
        params: ConfigGetRequest,
    ) -> AgentFuture<'_, TalosResult<ConfigResponse>> {
        Box::pin(async move {
            Self::actor(&context)?;
            self.config_service
                .as_ref()
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Worker configuration service is unavailable",
                    )
                })?
                .get_config(params)
        })
    }
    fn update_config(
        &self,
        context: WorkerEnvironmentContext,
        params: ConfigUpdateRequest,
    ) -> AgentFuture<'_, TalosResult<ConfigResponse>> {
        Box::pin(async move {
            Self::actor(&context)?;
            self.config_service
                .as_ref()
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Worker configuration service is unavailable",
                    )
                })?
                .update_config(params)
                .await
        })
    }
    fn delete_config(
        &self,
        context: WorkerEnvironmentContext,
        params: ConfigDeleteRequest,
    ) -> AgentFuture<'_, TalosResult<ConfigDeleteResponse>> {
        Box::pin(async move {
            Self::actor(&context)?;
            self.config_service
                .as_ref()
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Worker configuration service is unavailable",
                    )
                })?
                .delete_config(params)
        })
    }
    fn reload_config(
        &self,
        context: WorkerEnvironmentContext,
        params: ConfigReloadRequest,
    ) -> AgentFuture<'_, TalosResult<ConfigReloadResponse>> {
        Box::pin(async move {
            Self::actor(&context)?;
            self.config_service
                .as_ref()
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Worker configuration service is unavailable",
                    )
                })?
                .reload_config(params)
                .await
        })
    }

    fn list_memory_files(
        &self,
        context: WorkerEnvironmentContext,
        params: MemoryFileListRequest,
    ) -> AgentFuture<'_, TalosResult<MemoryFileListResponse>> {
        Box::pin(async move {
            self.memory(&context)?
                .list_files_with_request(&Self::actor(&context)?, params)
                .await
        })
    }
    fn read_memory_file(
        &self,
        context: WorkerEnvironmentContext,
        params: MemoryFileReadRequest,
    ) -> AgentFuture<'_, TalosResult<MemoryFileReadResponse>> {
        Box::pin(async move {
            Ok(MemoryFileReadResponse {
                file: self
                    .memory(&context)?
                    .read_file_with_request(&Self::actor(&context)?, params)
                    .await?,
            })
        })
    }
    fn write_memory_file(
        &self,
        context: WorkerEnvironmentContext,
        params: MemoryFileWriteRequest,
    ) -> AgentFuture<'_, TalosResult<MemoryFileWriteResponse>> {
        Box::pin(async move {
            Ok(MemoryFileWriteResponse {
                file: self
                    .memory(&context)?
                    .write_file_with_request(&Self::actor(&context)?, params)
                    .await?,
            })
        })
    }
    fn search_memory(
        &self,
        context: WorkerEnvironmentContext,
        params: MemorySearchRequest,
    ) -> AgentFuture<'_, TalosResult<MemorySearchResponse>> {
        Box::pin(async move {
            self.memory(&context)?
                .search(&Self::actor(&context)?, params)
                .await
        })
    }
    fn get_memory(
        &self,
        context: WorkerEnvironmentContext,
        params: MemoryGetRequest,
    ) -> AgentFuture<'_, TalosResult<MemoryGetResponse>> {
        Box::pin(async move {
            self.memory(&context)?
                .get(&Self::actor(&context)?, params)
                .await
        })
    }
    fn list_skills(
        &self,
        context: WorkerEnvironmentContext,
        params: SkillsListRequest,
    ) -> AgentFuture<'_, TalosResult<SkillsListResponse>> {
        Box::pin(async move { self.skills(&context)?.list_with_request(params) })
    }
    fn refresh_skills(
        &self,
        context: WorkerEnvironmentContext,
        params: SkillsRefreshRequest,
    ) -> AgentFuture<'_, TalosResult<SkillsListResponse>> {
        Box::pin(async move { self.skills(&context)?.refresh_with_request(params) })
    }
    fn suggest_skills(
        &self,
        context: WorkerEnvironmentContext,
        params: SkillsSuggestRequest,
    ) -> AgentFuture<'_, TalosResult<SkillsSuggestResponse>> {
        Box::pin(async move { self.skills(&context)?.suggest_with_request(params) })
    }
    fn read_skill(
        &self,
        context: WorkerEnvironmentContext,
        params: SkillsReadRequest,
    ) -> AgentFuture<'_, TalosResult<SkillsReadResponse>> {
        Box::pin(async move {
            Ok(SkillsReadResponse {
                document: self.skills(&context)?.read_with_request(params)?,
            })
        })
    }
    fn resolve_skills(
        &self,
        context: WorkerEnvironmentContext,
        params: SkillsResolveRequest,
    ) -> AgentFuture<'_, TalosResult<SkillsResolveResponse>> {
        Box::pin(async move { self.skills(&context)?.resolve_with_request(params) })
    }

    fn list_workspace_docs(
        &self,
        context: WorkerEnvironmentContext,
        params: WorkspaceDocListRequest,
    ) -> AgentFuture<'_, TalosResult<WorkspaceDocListResponse>> {
        Box::pin(async move {
            self.docs(&context)?
                .list_docs_with_request(&Self::actor(&context)?, params)
                .await
        })
    }
    fn read_workspace_doc(
        &self,
        context: WorkerEnvironmentContext,
        params: WorkspaceDocReadRequest,
    ) -> AgentFuture<'_, TalosResult<WorkspaceDocReadResponse>> {
        Box::pin(async move {
            Ok(WorkspaceDocReadResponse {
                doc: self
                    .docs(&context)?
                    .read_doc_with_request(&Self::actor(&context)?, params)
                    .await?,
            })
        })
    }
    fn write_workspace_doc(
        &self,
        context: WorkerEnvironmentContext,
        params: WorkspaceDocWriteRequest,
    ) -> AgentFuture<'_, TalosResult<WorkspaceDocWriteResponse>> {
        Box::pin(async move {
            Ok(WorkspaceDocWriteResponse {
                doc: self
                    .docs(&context)?
                    .write_doc_with_request(&Self::actor(&context)?, params)
                    .await?,
            })
        })
    }

    fn workspace_info(
        &self,
        context: WorkerEnvironmentContext,
    ) -> AgentFuture<'_, TalosResult<WorkspaceInfoResponse>> {
        Box::pin(async move {
            Self::actor(&context)?;
            let config = self.snapshot();
            let id = context
                .workspace_id
                .as_deref()
                .unwrap_or_else(|| config.default_workspace_id());
            let workspace = config.resolve_workspace(id, &context.user_id)?;
            Ok(WorkspaceInfoResponse {
                workspace_id: workspace.workspace_id,
                root_prefix: config
                    .workspaces
                    .root_prefix(&config.talos_paths(), id)?
                    .to_string_lossy()
                    .into_owned(),
                user_workspace_dir: workspace.root.to_string_lossy().into_owned(),
            })
        })
    }
    fn list_models(
        &self,
        context: WorkerEnvironmentContext,
    ) -> AgentFuture<'_, TalosResult<ModelListResponse>> {
        Box::pin(async move {
            Self::actor(&context)?;
            let config = self.snapshot();
            Ok(ModelListResponse {
                models: resolve_model_catalog(&config.llm),
                default_model_selection: resolve_default_model_selection(&config.llm)?,
            })
        })
    }
    fn preview_resource(
        &self,
        context: WorkerEnvironmentContext,
        params: WorkerPreviewRequest,
    ) -> AgentFuture<'_, TalosResult<WorkerPreviewResponse>> {
        Box::pin(async move {
            Self::actor(&context)?;
            self.preview_roots.operate(context.user_id, params).await
        })
    }
    fn read_directory(
        &self,
        context: WorkerEnvironmentContext,
        params: FsDirectoryReadRequest,
    ) -> AgentFuture<'_, TalosResult<FsDirectorySnapshot>> {
        Box::pin(async move {
            self.fs()
                .read_directory(&Self::actor(&context)?, params.path)
                .await
        })
    }
    fn create_directory(
        &self,
        context: WorkerEnvironmentContext,
        params: FsDirectoryCreateRequest,
    ) -> AgentFuture<'_, TalosResult<FsDirectoryCreateResponse>> {
        Box::pin(async move {
            self.fs()
                .create_directory(&Self::actor(&context)?, params.parent, params.name)
                .await
        })
    }
    fn project_status(
        &self,
        context: WorkerEnvironmentContext,
        directory: PathBuf,
    ) -> AgentFuture<'_, TalosResult<ProjectStatus>> {
        Box::pin(async move {
            Self::actor(&context)?;
            Ok(self.fs().project_status_for_directory(&directory))
        })
    }
    fn validate_project_directory(
        &self,
        context: WorkerEnvironmentContext,
        directory: PathBuf,
    ) -> AgentFuture<'_, TalosResult<PathBuf>> {
        Box::pin(async move {
            Self::actor(&context)?;
            validate_project_directory(&directory)?;
            let directory = talos_fs::canonicalize_path(&directory)?;
            talos_fs::read_directory(&directory)?;
            Ok(directory)
        })
    }
}
