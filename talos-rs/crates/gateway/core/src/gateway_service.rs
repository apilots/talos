use std::sync::Arc;

use async_trait::async_trait;
use talos_agent_api::{
    InterruptTurnRequest, InterruptTurnResponse, RecoverTimelineResponse,
    SessionActiveTurnResponse, SessionHistoryResponse, SubmitTurnInputRequest,
    SubmitTurnInputResponse,
};
use talos_auth::{
    CloudLoginCancelRequest, CloudLoginPersistence, CloudLoginStartRequest,
    CloudLoginStartResponse, CloudLoginStatusRequest, CloudLoginStatusResponse,
};
use talos_error::TalosResult;
use talos_service::{Actor, FileByteStream, TalosService};
use talos_types::{
    AuthLogoutRequest, AuthLogoutResponse, AuthSessionClearRequest, AuthSessionClearResponse,
    AuthSessionReadRequest, AuthSessionReadResponse, CloudLoginCancelResponse, ConfigDeleteRequest,
    ConfigDeleteResponse, ConfigGetRequest, ConfigReloadRequest, ConfigReloadResponse,
    ConfigResponse, ConfigUpdateRequest, FileDeleteRequest, FileDeleteResponse, FileListRequest,
    FileListResponse, FileReadRequest, FileReadResponse, FileUpdateRequest, FileUpdateResponse,
    FileUploadRequest, FileUploadResponse, FsDirectoryCreateRequest, FsDirectoryCreateResponse,
    FsDirectoryReadRequest, FsDirectoryReadResponse, MemoryFileListRequest, MemoryFileListResponse,
    MemoryFileReadRequest, MemoryFileReadResponse, MemoryFileWriteRequest, MemoryFileWriteResponse,
    MemoryGetRequest, MemoryGetResponse, MemorySearchRequest, MemorySearchResponse, MessageQueue,
    MessageQueueAddRequest, MessageQueueDeleteRequest, MessageQueueListRequest,
    MessageQueueReorderRequest, MessageQueueStartRequest, MessageQueueUpdateRequest,
    ModelListRequest, ModelListResponse, PluginDisableRequest, PluginDisableResponse,
    PluginEnableRequest, PluginEnableResponse, PluginInstallRequest, PluginInstallResponse,
    PluginListRequest, PluginListResponse, PluginReadRequest, PluginReadResponse,
    PluginSearchRequest, PluginSearchResponse, PluginSourceAddRequest, PluginSourceAddResponse,
    PluginSourceListRequest, PluginSourceListResponse, PluginSourceRefreshRequest,
    PluginSourceRefreshResponse, PluginSourceRemoveRequest, PluginSourceRemoveResponse,
    PluginUninstallRequest, PluginUninstallResponse, PluginUpdateRequest, PluginUpdateResponse,
    ProfileReadRequest, ProfileReadResponse, ProjectCreateRequest, ProjectCreateResponse,
    ProjectDeleteRequest, ProjectDeleteResponse, ProjectListRequest, ProjectListResponse,
    ProjectPatchRequest, ProjectPatchResponse, ProjectReadRequest, ProjectReadResponse,
    RecoverTimelineRequest, RetryTurnRequest, SessionActiveTurnRequest, SessionClearRequest,
    SessionClearResponse, SessionCloseRequest, SessionCloseResponse, SessionCompactRequest,
    SessionCompactResponse, SessionDeleteRequest, SessionDeleteResponse, SessionGetRequest,
    SessionGetResponse, SessionHistoryRequest, SessionListRequest, SessionListResponse,
    SessionPatchRequest, SessionUpdateResponse, SkillsListRequest, SkillsListResponse,
    SkillsReadRequest, SkillsReadResponse, SkillsRefreshRequest, SkillsResolveRequest,
    SkillsResolveResponse, SkillsSuggestRequest, SkillsSuggestResponse, StartTurnRequest,
    StartTurnResponse, SubscribeTimelineRequest, SubscribeTimelineResponse, UserCurrentRequest,
    UserCurrentResponse, WorkspaceDocListRequest, WorkspaceDocListResponse,
    WorkspaceDocReadRequest, WorkspaceDocReadResponse, WorkspaceDocWriteRequest,
    WorkspaceDocWriteResponse, WorkspaceInfoRequest, WorkspaceInfoResponse,
};
use tracing::info;

use crate::gateway::Gateway;

pub struct ServiceGateway {
    pub(crate) service: Arc<TalosService>,
}

impl ServiceGateway {
    pub fn new(service: Arc<TalosService>) -> Self {
        Self { service }
    }

    pub(crate) async fn start(&self) -> TalosResult<()> {
        info!("Starting gateway...");
        Ok(())
    }
}

pub(crate) fn require_non_empty(value: &str, field: &str) -> TalosResult<()> {
    if value.trim().is_empty() {
        return Err(talos_error::TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("{field} is required"),
        ));
    }
    Ok(())
}

#[async_trait]
impl Gateway for ServiceGateway {
    async fn list_computers(
        &self,
        actor: &Actor,
        request: talos_types::ComputerListRequest,
    ) -> TalosResult<talos_types::ComputerListResponse> {
        ServiceGateway::list_computers(self, actor, request).await
    }
    async fn read_computer(
        &self,
        actor: &Actor,
        request: talos_types::GetNodeRequest,
    ) -> TalosResult<talos_types::NodeInfo> {
        ServiceGateway::read_computer(self, actor, request).await
    }
    async fn create_computer(
        &self,
        actor: &Actor,
        request: talos_types::CreateNodeRequest,
    ) -> TalosResult<talos_types::CreatedNode> {
        ServiceGateway::create_computer(self, actor, request).await
    }
    async fn rename_computer(
        &self,
        actor: &Actor,
        request: talos_types::RenameComputerRequest,
    ) -> TalosResult<talos_types::NodeInfo> {
        ServiceGateway::rename_computer(self, actor, request).await
    }
    async fn delete_computer(
        &self,
        actor: &Actor,
        request: talos_types::DeleteComputerRequest,
    ) -> TalosResult<talos_types::DeleteComputerResponse> {
        ServiceGateway::delete_computer(self, actor, request).await
    }

    async fn set_computer_status(
        &self,
        actor: &Actor,
        request: talos_types::SetComputerStatusRequest,
    ) -> TalosResult<talos_types::NodeInfo> {
        ServiceGateway::set_computer_status(self, actor, request).await
    }
    async fn rotate_computer_credential(
        &self,
        actor: &Actor,
        request: talos_types::RotateCredentialRequest,
    ) -> TalosResult<talos_types::NodeCredential> {
        ServiceGateway::rotate_computer_credential(self, actor, request).await
    }

    async fn read_profile(
        &self,
        actor: &Actor,
        request: ProfileReadRequest,
    ) -> TalosResult<ProfileReadResponse> {
        ServiceGateway::read_profile(self, actor, request).await
    }

    async fn start_turn(
        &self,
        actor: &Actor,
        source: talos_types::talos::SessionSource,
        request: StartTurnRequest,
        idempotency_key: Option<String>,
    ) -> TalosResult<StartTurnResponse> {
        ServiceGateway::start_turn(self, actor, source, request, idempotency_key).await
    }

    async fn retry_turn(
        &self,
        actor: &Actor,
        request: RetryTurnRequest,
    ) -> TalosResult<StartTurnResponse> {
        ServiceGateway::retry_turn(self, actor, request).await
    }

    async fn interrupt_turn(
        &self,
        actor: &Actor,
        request: InterruptTurnRequest,
    ) -> TalosResult<InterruptTurnResponse> {
        ServiceGateway::interrupt_turn(self, actor, request).await
    }

    async fn add_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueAddRequest,
    ) -> TalosResult<MessageQueue> {
        ServiceGateway::add_queued_message(self, actor, request).await
    }

    async fn list_message_queue(
        &self,
        actor: &Actor,
        request: MessageQueueListRequest,
    ) -> TalosResult<MessageQueue> {
        ServiceGateway::list_message_queue(self, actor, request).await
    }

    async fn update_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueUpdateRequest,
    ) -> TalosResult<MessageQueue> {
        ServiceGateway::update_queued_message(self, actor, request).await
    }

    async fn delete_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueDeleteRequest,
    ) -> TalosResult<MessageQueue> {
        ServiceGateway::delete_queued_message(self, actor, request).await
    }

    async fn reorder_message_queue(
        &self,
        actor: &Actor,
        request: MessageQueueReorderRequest,
    ) -> TalosResult<MessageQueue> {
        ServiceGateway::reorder_message_queue(self, actor, request).await
    }

    async fn start_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueStartRequest,
    ) -> TalosResult<StartTurnResponse> {
        ServiceGateway::start_queued_message(self, actor, request).await
    }

    async fn submit_turn_input(
        &self,
        actor: &Actor,
        request: SubmitTurnInputRequest,
    ) -> TalosResult<SubmitTurnInputResponse> {
        ServiceGateway::submit_turn_input(self, actor, request).await
    }

    async fn subscribe_timeline(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<SubscribeTimelineResponse> {
        ServiceGateway::subscribe_timeline(self, actor, request).await
    }

    async fn recover_timeline(
        &self,
        actor: &Actor,
        request: RecoverTimelineRequest,
    ) -> TalosResult<RecoverTimelineResponse> {
        ServiceGateway::recover_timeline(self, actor, request).await
    }

    async fn list_sessions(
        &self,
        actor: &Actor,
        request: SessionListRequest,
    ) -> TalosResult<SessionListResponse> {
        ServiceGateway::list_sessions(self, actor, request).await
    }

    async fn get_session(
        &self,
        actor: &Actor,
        request: SessionGetRequest,
    ) -> TalosResult<SessionGetResponse> {
        ServiceGateway::get_session(self, actor, request).await
    }

    async fn get_session_history(
        &self,
        actor: &Actor,
        request: SessionHistoryRequest,
    ) -> TalosResult<SessionHistoryResponse> {
        ServiceGateway::get_session_history(self, actor, request).await
    }

    async fn get_active_turn(
        &self,
        actor: &Actor,
        request: SessionActiveTurnRequest,
    ) -> TalosResult<SessionActiveTurnResponse> {
        ServiceGateway::get_active_turn(self, actor, request).await
    }

    async fn patch_session(
        &self,
        actor: &Actor,
        request: SessionPatchRequest,
    ) -> TalosResult<SessionUpdateResponse> {
        ServiceGateway::patch_session(self, actor, request).await
    }

    async fn close_session(
        &self,
        actor: &Actor,
        request: SessionCloseRequest,
    ) -> TalosResult<SessionCloseResponse> {
        ServiceGateway::close_session(self, actor, request).await
    }

    async fn delete_session(
        &self,
        actor: &Actor,
        request: SessionDeleteRequest,
    ) -> TalosResult<SessionDeleteResponse> {
        ServiceGateway::delete_session(self, actor, request).await
    }

    async fn compact_session(
        &self,
        actor: &Actor,
        request: SessionCompactRequest,
    ) -> TalosResult<SessionCompactResponse> {
        ServiceGateway::compact_session(self, actor, request).await
    }

    async fn clear_session(
        &self,
        actor: &Actor,
        request: SessionClearRequest,
    ) -> TalosResult<SessionClearResponse> {
        ServiceGateway::clear_session(self, actor, request).await
    }

    async fn list_projects(
        &self,
        actor: &Actor,
        request: ProjectListRequest,
    ) -> TalosResult<ProjectListResponse> {
        ServiceGateway::list_projects(self, actor, request).await
    }

    async fn get_project(
        &self,
        actor: &Actor,
        request: ProjectReadRequest,
    ) -> TalosResult<ProjectReadResponse> {
        ServiceGateway::get_project(self, actor, request).await
    }

    async fn create_project(
        &self,
        actor: &Actor,
        request: ProjectCreateRequest,
    ) -> TalosResult<ProjectCreateResponse> {
        ServiceGateway::create_project(self, actor, request).await
    }

    async fn patch_project(
        &self,
        actor: &Actor,
        request: ProjectPatchRequest,
    ) -> TalosResult<ProjectPatchResponse> {
        ServiceGateway::patch_project(self, actor, request).await
    }

    async fn delete_project(
        &self,
        actor: &Actor,
        request: ProjectDeleteRequest,
    ) -> TalosResult<ProjectDeleteResponse> {
        ServiceGateway::delete_project(self, actor, request).await
    }

    async fn read_fs_directory(
        &self,
        actor: &Actor,
        request: FsDirectoryReadRequest,
    ) -> TalosResult<FsDirectoryReadResponse> {
        ServiceGateway::read_fs_directory(self, actor, request).await
    }

    async fn create_fs_directory(
        &self,
        actor: &Actor,
        request: FsDirectoryCreateRequest,
    ) -> TalosResult<FsDirectoryCreateResponse> {
        ServiceGateway::create_fs_directory(self, actor, request).await
    }

    async fn list_models(
        &self,
        actor: &Actor,
        request: ModelListRequest,
    ) -> TalosResult<ModelListResponse> {
        ServiceGateway::list_models(self, actor, request).await
    }

    async fn get_config(
        &self,
        actor: &Actor,
        request: ConfigGetRequest,
    ) -> TalosResult<ConfigResponse> {
        ServiceGateway::get_config(self, actor, request).await
    }

    async fn update_config(
        &self,
        actor: &Actor,
        request: ConfigUpdateRequest,
    ) -> TalosResult<ConfigResponse> {
        ServiceGateway::update_config(self, actor, request).await
    }

    async fn delete_config(
        &self,
        actor: &Actor,
        request: ConfigDeleteRequest,
    ) -> TalosResult<ConfigDeleteResponse> {
        ServiceGateway::delete_config(self, actor, request).await
    }

    async fn reload_config(
        &self,
        actor: &Actor,
        request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse> {
        ServiceGateway::reload_config(self, actor, request).await
    }

    async fn list_skills(
        &self,
        actor: &Actor,
        request: SkillsListRequest,
    ) -> TalosResult<SkillsListResponse> {
        ServiceGateway::list_skills(self, actor, request).await
    }

    async fn refresh_skills(
        &self,
        actor: &Actor,
        request: SkillsRefreshRequest,
    ) -> TalosResult<SkillsListResponse> {
        ServiceGateway::refresh_skills(self, actor, request).await
    }

    async fn suggest_skills(
        &self,
        actor: &Actor,
        request: SkillsSuggestRequest,
    ) -> TalosResult<SkillsSuggestResponse> {
        ServiceGateway::suggest_skills(self, actor, request).await
    }

    async fn read_skill(
        &self,
        actor: &Actor,
        request: SkillsReadRequest,
    ) -> TalosResult<SkillsReadResponse> {
        ServiceGateway::read_skill(self, actor, request).await
    }

    async fn resolve_skills(
        &self,
        actor: &Actor,
        request: SkillsResolveRequest,
    ) -> TalosResult<SkillsResolveResponse> {
        ServiceGateway::resolve_skills(self, actor, request).await
    }

    async fn list_plugin_sources(
        &self,
        actor: &Actor,
        request: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse> {
        ServiceGateway::list_plugin_sources(self, actor, request).await
    }

    async fn add_plugin_source(
        &self,
        actor: &Actor,
        request: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse> {
        ServiceGateway::add_plugin_source(self, actor, request).await
    }

    async fn refresh_plugin_source(
        &self,
        actor: &Actor,
        request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        ServiceGateway::refresh_plugin_source(self, actor, request).await
    }

    async fn remove_plugin_source(
        &self,
        actor: &Actor,
        request: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse> {
        ServiceGateway::remove_plugin_source(self, actor, request).await
    }

    async fn search_plugins(
        &self,
        actor: &Actor,
        request: PluginSearchRequest,
    ) -> TalosResult<PluginSearchResponse> {
        ServiceGateway::search_plugins(self, actor, request).await
    }

    async fn import_mcp_server(
        &self,
        actor: &Actor,
        request: talos_types::McpServerImportRequest,
    ) -> TalosResult<talos_types::PluginInstallResponse> {
        self.service.import_mcp_server(actor, request).await
    }

    async fn start_mcp_authentication(
        &self,
        actor: &Actor,
        request: talos_types::McpAuthenticationStartRequest,
    ) -> TalosResult<talos_types::McpAuthenticationStartResponse> {
        self.service.start_mcp_authentication(actor, request).await
    }

    async fn read_mcp_authentication(
        &self,
        actor: &Actor,
        request: talos_types::McpAuthenticationReadRequest,
    ) -> TalosResult<talos_types::McpAuthenticationResponse> {
        self.service.read_mcp_authentication(actor, request).await
    }

    async fn update_mcp_credentials(
        &self,
        actor: &Actor,
        request: talos_types::McpCredentialsUpdateRequest,
    ) -> TalosResult<talos_types::McpAuthenticationResponse> {
        self.service.update_mcp_credentials(actor, request).await
    }

    async fn list_mcp_servers(
        &self,
        actor: &Actor,
        request: talos_types::McpServerListRequest,
    ) -> TalosResult<talos_types::McpServerListResponse> {
        self.service.list_mcp_servers(actor, request).await
    }

    async fn list_plugins(
        &self,
        actor: &Actor,
        request: PluginListRequest,
    ) -> TalosResult<PluginListResponse> {
        ServiceGateway::list_plugins(self, actor, request).await
    }

    async fn read_plugin(
        &self,
        actor: &Actor,
        request: PluginReadRequest,
    ) -> TalosResult<PluginReadResponse> {
        ServiceGateway::read_plugin(self, actor, request).await
    }

    async fn install_plugin(
        &self,
        actor: &Actor,
        request: PluginInstallRequest,
    ) -> TalosResult<PluginInstallResponse> {
        ServiceGateway::install_plugin(self, actor, request).await
    }

    async fn update_plugin(
        &self,
        actor: &Actor,
        request: PluginUpdateRequest,
    ) -> TalosResult<PluginUpdateResponse> {
        ServiceGateway::update_plugin(self, actor, request).await
    }

    async fn uninstall_plugin(
        &self,
        actor: &Actor,
        request: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse> {
        ServiceGateway::uninstall_plugin(self, actor, request).await
    }

    async fn enable_plugin(
        &self,
        actor: &Actor,
        request: PluginEnableRequest,
    ) -> TalosResult<PluginEnableResponse> {
        ServiceGateway::enable_plugin(self, actor, request).await
    }

    async fn disable_plugin(
        &self,
        actor: &Actor,
        request: PluginDisableRequest,
    ) -> TalosResult<PluginDisableResponse> {
        ServiceGateway::disable_plugin(self, actor, request).await
    }

    async fn upload_file(
        &self,
        actor: &Actor,
        request: FileUploadRequest,
        body: FileByteStream,
    ) -> TalosResult<FileUploadResponse> {
        ServiceGateway::upload_file(self, actor, request, body).await
    }

    async fn list_files(
        &self,
        actor: &Actor,
        request: FileListRequest,
    ) -> TalosResult<FileListResponse> {
        ServiceGateway::list_files(self, actor, request).await
    }

    async fn read_file(
        &self,
        actor: &Actor,
        request: FileReadRequest,
    ) -> TalosResult<FileReadResponse> {
        ServiceGateway::read_file(self, actor, request).await
    }

    async fn read_file_content(
        &self,
        actor: &Actor,
        request: FileReadRequest,
    ) -> TalosResult<talos_types::FileContentResponse> {
        ServiceGateway::read_file_content(self, actor, request).await
    }

    async fn update_file(
        &self,
        actor: &Actor,
        request: FileUpdateRequest,
    ) -> TalosResult<FileUpdateResponse> {
        ServiceGateway::update_file(self, actor, request).await
    }

    async fn delete_file(
        &self,
        actor: &Actor,
        request: FileDeleteRequest,
    ) -> TalosResult<FileDeleteResponse> {
        ServiceGateway::delete_file(self, actor, request).await
    }

    async fn list_memory_files(
        &self,
        actor: &Actor,
        request: MemoryFileListRequest,
    ) -> TalosResult<MemoryFileListResponse> {
        ServiceGateway::list_memory_files(self, actor, request).await
    }

    async fn read_memory_file(
        &self,
        actor: &Actor,
        request: MemoryFileReadRequest,
    ) -> TalosResult<MemoryFileReadResponse> {
        ServiceGateway::read_memory_file(self, actor, request).await
    }

    async fn write_memory_file(
        &self,
        actor: &Actor,
        request: MemoryFileWriteRequest,
    ) -> TalosResult<MemoryFileWriteResponse> {
        ServiceGateway::write_memory_file(self, actor, request).await
    }

    async fn search_memory(
        &self,
        actor: &Actor,
        request: MemorySearchRequest,
    ) -> TalosResult<MemorySearchResponse> {
        ServiceGateway::search_memory(self, actor, request).await
    }

    async fn get_memory(
        &self,
        actor: &Actor,
        request: MemoryGetRequest,
    ) -> TalosResult<MemoryGetResponse> {
        ServiceGateway::get_memory(self, actor, request).await
    }

    async fn workspace_info(
        &self,
        actor: &Actor,
        request: WorkspaceInfoRequest,
    ) -> TalosResult<WorkspaceInfoResponse> {
        ServiceGateway::workspace_info(self, actor, request).await
    }

    async fn list_workspace_docs(
        &self,
        actor: &Actor,
        request: WorkspaceDocListRequest,
    ) -> TalosResult<WorkspaceDocListResponse> {
        ServiceGateway::list_workspace_docs(self, actor, request).await
    }

    async fn read_workspace_doc(
        &self,
        actor: &Actor,
        request: WorkspaceDocReadRequest,
    ) -> TalosResult<WorkspaceDocReadResponse> {
        ServiceGateway::read_workspace_doc(self, actor, request).await
    }

    async fn write_workspace_doc(
        &self,
        actor: &Actor,
        request: WorkspaceDocWriteRequest,
    ) -> TalosResult<WorkspaceDocWriteResponse> {
        ServiceGateway::write_workspace_doc(self, actor, request).await
    }

    async fn logout(
        &self,
        actor: &Actor,
        request: AuthLogoutRequest,
    ) -> TalosResult<AuthLogoutResponse> {
        ServiceGateway::logout(self, actor, request).await
    }

    async fn start_cloud_login(
        &self,
        actor: &Actor,
        request: CloudLoginStartRequest,
        persistence: CloudLoginPersistence,
    ) -> TalosResult<CloudLoginStartResponse> {
        ServiceGateway::start_cloud_login(self, actor, request, persistence).await
    }

    async fn cloud_login_status(
        &self,
        actor: &Actor,
        request: CloudLoginStatusRequest,
    ) -> TalosResult<CloudLoginStatusResponse> {
        ServiceGateway::cloud_login_status(self, actor, request).await
    }

    async fn cancel_cloud_login(
        &self,
        actor: &Actor,
        request: CloudLoginCancelRequest,
    ) -> TalosResult<CloudLoginCancelResponse> {
        ServiceGateway::cancel_cloud_login(self, actor, request).await
    }

    async fn read_auth_session(
        &self,
        actor: &Actor,
        request: AuthSessionReadRequest,
    ) -> TalosResult<AuthSessionReadResponse> {
        ServiceGateway::read_auth_session(self, actor, request).await
    }

    async fn clear_auth_session(
        &self,
        actor: &Actor,
        request: AuthSessionClearRequest,
    ) -> TalosResult<AuthSessionClearResponse> {
        ServiceGateway::clear_auth_session(self, actor, request).await
    }

    async fn current_user(
        &self,
        actor: &Actor,
        request: UserCurrentRequest,
    ) -> TalosResult<UserCurrentResponse> {
        ServiceGateway::current_user(self, actor, request).await
    }

    async fn start(&self) -> TalosResult<()> {
        ServiceGateway::start(self).await
    }
}
