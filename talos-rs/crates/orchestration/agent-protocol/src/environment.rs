use std::path::PathBuf;

use talos_types::{
    FsDirectoryCreateRequest, FsDirectoryCreateResponse, FsDirectoryReadRequest,
    FsDirectorySnapshot, MemoryFileListRequest, MemoryFileListResponse, MemoryFileReadRequest,
    MemoryFileReadResponse, MemoryFileWriteRequest, MemoryFileWriteResponse, MemoryGetRequest,
    MemoryGetResponse, MemorySearchRequest, MemorySearchResponse, ModelListResponse, ProjectStatus,
    SkillsListRequest, SkillsListResponse, SkillsReadRequest, SkillsReadResponse,
    SkillsRefreshRequest, SkillsResolveRequest, SkillsResolveResponse, SkillsSuggestRequest,
    SkillsSuggestResponse, WorkspaceDocListRequest, WorkspaceDocListResponse,
    WorkspaceDocReadRequest, WorkspaceDocReadResponse, WorkspaceDocWriteRequest,
    WorkspaceDocWriteResponse, WorkspaceInfoResponse,
};

use crate::{AgentFuture, AgentResult};

/// Trusted actor and optional saved Workspace binding; never a fabricated Session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerEnvironmentContext {
    pub user_id: String,
    pub username: Option<String>,
    /// None selects the Worker's configured default. An explicit ID must exist.
    pub workspace_id: Option<String>,
}

/// Node-local environment owner. Clients hide local/direct/outbound transport.
/// Callers authorize the selected node; network adapters additionally verify its lease
/// and actor. Implementations own path validation and I/O, never product persistence.
pub trait WorkerEnvironment: Send + Sync {
    fn list_plugin_sources(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::PluginSourceListRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::PluginSourceListResponse>>;
    fn add_plugin_source(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::PluginSourceAddRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::PluginSourceAddResponse>>;
    fn refresh_plugin_source(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::PluginSourceRefreshRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::PluginSourceRefreshResponse>>;
    fn remove_plugin_source(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::PluginSourceRemoveRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::PluginSourceRemoveResponse>>;
    fn search_plugins(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::PluginSearchRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::PluginSearchResponse>>;
    fn list_plugins(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::PluginListRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::PluginListResponse>>;
    fn read_plugin(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::PluginReadRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::PluginReadResponse>>;
    fn install_plugin(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::PluginInstallRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::PluginInstallResponse>>;
    fn update_plugin(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::PluginUpdateRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::PluginUpdateResponse>>;
    fn uninstall_plugin(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::PluginUninstallRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::PluginUninstallResponse>>;
    fn enable_plugin(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::PluginEnableRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::PluginEnableResponse>>;
    fn disable_plugin(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::PluginDisableRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::PluginDisableResponse>>;
    fn list_mcp_servers(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::McpServerListRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::McpServerListResponse>>;
    fn read_mcp_authentication(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::McpAuthenticationReadRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::McpAuthenticationResponse>>;
    fn update_mcp_credentials(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::McpCredentialsUpdateRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::McpAuthenticationResponse>>;
    fn import_mcp_server(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::McpServerImportRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::PluginInstallResponse>>;
    fn start_mcp_authentication(
        &self,
        context: WorkerEnvironmentContext,
        params: crate::McpAuthenticationStartParams,
    ) -> AgentFuture<'_, AgentResult<talos_types::McpAuthenticationStartResponse>>;

    fn complete_mcp_authentication(
        &self,
        context: WorkerEnvironmentContext,
        params: crate::McpAuthenticationCallbackParams,
    ) -> AgentFuture<'_, AgentResult<talos_types::McpAuthenticationResponse>>;

    fn get_config(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::ConfigGetRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::ConfigResponse>>;
    fn update_config(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::ConfigUpdateRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::ConfigResponse>>;
    fn delete_config(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::ConfigDeleteRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::ConfigDeleteResponse>>;
    fn reload_config(
        &self,
        context: WorkerEnvironmentContext,
        params: talos_types::ConfigReloadRequest,
    ) -> AgentFuture<'_, AgentResult<talos_types::ConfigReloadResponse>>;

    fn list_memory_files(
        &self,
        context: WorkerEnvironmentContext,
        params: MemoryFileListRequest,
    ) -> AgentFuture<'_, AgentResult<MemoryFileListResponse>>;
    fn read_memory_file(
        &self,
        context: WorkerEnvironmentContext,
        params: MemoryFileReadRequest,
    ) -> AgentFuture<'_, AgentResult<MemoryFileReadResponse>>;
    fn write_memory_file(
        &self,
        context: WorkerEnvironmentContext,
        params: MemoryFileWriteRequest,
    ) -> AgentFuture<'_, AgentResult<MemoryFileWriteResponse>>;
    fn search_memory(
        &self,
        context: WorkerEnvironmentContext,
        params: MemorySearchRequest,
    ) -> AgentFuture<'_, AgentResult<MemorySearchResponse>>;
    fn get_memory(
        &self,
        context: WorkerEnvironmentContext,
        params: MemoryGetRequest,
    ) -> AgentFuture<'_, AgentResult<MemoryGetResponse>>;
    fn list_skills(
        &self,
        context: WorkerEnvironmentContext,
        params: SkillsListRequest,
    ) -> AgentFuture<'_, AgentResult<SkillsListResponse>>;
    fn refresh_skills(
        &self,
        context: WorkerEnvironmentContext,
        params: SkillsRefreshRequest,
    ) -> AgentFuture<'_, AgentResult<SkillsListResponse>>;
    fn suggest_skills(
        &self,
        context: WorkerEnvironmentContext,
        params: SkillsSuggestRequest,
    ) -> AgentFuture<'_, AgentResult<SkillsSuggestResponse>>;
    fn read_skill(
        &self,
        context: WorkerEnvironmentContext,
        params: SkillsReadRequest,
    ) -> AgentFuture<'_, AgentResult<SkillsReadResponse>>;
    fn resolve_skills(
        &self,
        context: WorkerEnvironmentContext,
        params: SkillsResolveRequest,
    ) -> AgentFuture<'_, AgentResult<SkillsResolveResponse>>;

    fn list_workspace_docs(
        &self,
        context: WorkerEnvironmentContext,
        params: WorkspaceDocListRequest,
    ) -> AgentFuture<'_, AgentResult<WorkspaceDocListResponse>>;
    fn read_workspace_doc(
        &self,
        context: WorkerEnvironmentContext,
        params: WorkspaceDocReadRequest,
    ) -> AgentFuture<'_, AgentResult<WorkspaceDocReadResponse>>;
    fn write_workspace_doc(
        &self,
        context: WorkerEnvironmentContext,
        params: WorkspaceDocWriteRequest,
    ) -> AgentFuture<'_, AgentResult<WorkspaceDocWriteResponse>>;
    fn workspace_info(
        &self,
        context: WorkerEnvironmentContext,
    ) -> AgentFuture<'_, AgentResult<WorkspaceInfoResponse>>;
    fn list_models(
        &self,
        context: WorkerEnvironmentContext,
    ) -> AgentFuture<'_, AgentResult<ModelListResponse>>;
    fn read_directory(
        &self,
        context: WorkerEnvironmentContext,
        params: FsDirectoryReadRequest,
    ) -> AgentFuture<'_, AgentResult<FsDirectorySnapshot>>;
    fn create_directory(
        &self,
        context: WorkerEnvironmentContext,
        params: FsDirectoryCreateRequest,
    ) -> AgentFuture<'_, AgentResult<FsDirectoryCreateResponse>>;
    fn project_status(
        &self,
        context: WorkerEnvironmentContext,
        directory: PathBuf,
    ) -> AgentFuture<'_, AgentResult<ProjectStatus>>;
    /// Validate existence/access and return the node's canonical absolute directory.
    fn validate_project_directory(
        &self,
        context: WorkerEnvironmentContext,
        directory: PathBuf,
    ) -> AgentFuture<'_, AgentResult<PathBuf>>;
}
