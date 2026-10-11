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
use talos_error::{TalosError, TalosResult};
use talos_service::{Actor, FileByteStream};
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

#[async_trait]
pub trait Gateway: Send + Sync {
    async fn list_computers(
        &self,
        actor: &Actor,
        request: talos_types::ComputerListRequest,
    ) -> TalosResult<talos_types::ComputerListResponse>;

    async fn read_computer(
        &self,
        actor: &Actor,
        request: talos_types::GetNodeRequest,
    ) -> TalosResult<talos_types::NodeInfo>;

    async fn create_computer(
        &self,
        actor: &Actor,
        request: talos_types::CreateNodeRequest,
    ) -> TalosResult<talos_types::CreatedNode>;

    async fn rename_computer(
        &self,
        actor: &Actor,
        request: talos_types::RenameComputerRequest,
    ) -> TalosResult<talos_types::NodeInfo>;

    async fn set_computer_status(
        &self,
        actor: &Actor,
        request: talos_types::SetComputerStatusRequest,
    ) -> TalosResult<talos_types::NodeInfo>;

    async fn rotate_computer_credential(
        &self,
        actor: &Actor,
        request: talos_types::RotateCredentialRequest,
    ) -> TalosResult<talos_types::NodeCredential>;

    /// Reads the authenticated caller's Profile usage view.
    async fn read_profile(
        &self,
        actor: &Actor,
        request: ProfileReadRequest,
    ) -> TalosResult<ProfileReadResponse>;

    async fn start_turn(
        &self,
        actor: &Actor,
        source: talos_types::talos::SessionSource,
        request: StartTurnRequest,
        idempotency_key: Option<String>,
    ) -> TalosResult<StartTurnResponse>;

    async fn retry_turn(
        &self,
        actor: &Actor,
        request: RetryTurnRequest,
    ) -> TalosResult<StartTurnResponse>;

    async fn interrupt_turn(
        &self,
        actor: &Actor,
        request: InterruptTurnRequest,
    ) -> TalosResult<InterruptTurnResponse>;

    async fn add_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueAddRequest,
    ) -> TalosResult<MessageQueue>;

    async fn list_message_queue(
        &self,
        actor: &Actor,
        request: MessageQueueListRequest,
    ) -> TalosResult<MessageQueue>;

    async fn update_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueUpdateRequest,
    ) -> TalosResult<MessageQueue>;

    async fn delete_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueDeleteRequest,
    ) -> TalosResult<MessageQueue>;

    async fn reorder_message_queue(
        &self,
        actor: &Actor,
        request: MessageQueueReorderRequest,
    ) -> TalosResult<MessageQueue>;

    async fn start_queued_message(
        &self,
        actor: &Actor,
        request: MessageQueueStartRequest,
    ) -> TalosResult<StartTurnResponse>;

    async fn submit_turn_input(
        &self,
        actor: &Actor,
        request: SubmitTurnInputRequest,
    ) -> TalosResult<SubmitTurnInputResponse>;

    async fn subscribe_timeline(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<SubscribeTimelineResponse>;

    async fn recover_timeline(
        &self,
        actor: &Actor,
        request: RecoverTimelineRequest,
    ) -> TalosResult<RecoverTimelineResponse>;

    async fn list_sessions(
        &self,
        actor: &Actor,
        request: SessionListRequest,
    ) -> TalosResult<SessionListResponse>;

    async fn get_session(
        &self,
        actor: &Actor,
        request: SessionGetRequest,
    ) -> TalosResult<SessionGetResponse>;

    async fn get_session_history(
        &self,
        actor: &Actor,
        request: SessionHistoryRequest,
    ) -> TalosResult<SessionHistoryResponse>;

    async fn get_active_turn(
        &self,
        actor: &Actor,
        request: SessionActiveTurnRequest,
    ) -> TalosResult<SessionActiveTurnResponse>;

    async fn patch_session(
        &self,
        actor: &Actor,
        request: SessionPatchRequest,
    ) -> TalosResult<SessionUpdateResponse>;

    async fn close_session(
        &self,
        actor: &Actor,
        request: SessionCloseRequest,
    ) -> TalosResult<SessionCloseResponse>;

    async fn delete_session(
        &self,
        actor: &Actor,
        request: SessionDeleteRequest,
    ) -> TalosResult<SessionDeleteResponse>;

    async fn compact_session(
        &self,
        actor: &Actor,
        request: SessionCompactRequest,
    ) -> TalosResult<SessionCompactResponse>;

    async fn clear_session(
        &self,
        actor: &Actor,
        request: SessionClearRequest,
    ) -> TalosResult<SessionClearResponse>;

    async fn list_projects(
        &self,
        actor: &Actor,
        request: ProjectListRequest,
    ) -> TalosResult<ProjectListResponse>;

    async fn get_project(
        &self,
        actor: &Actor,
        request: ProjectReadRequest,
    ) -> TalosResult<ProjectReadResponse>;

    async fn create_project(
        &self,
        actor: &Actor,
        request: ProjectCreateRequest,
    ) -> TalosResult<ProjectCreateResponse>;

    async fn patch_project(
        &self,
        actor: &Actor,
        request: ProjectPatchRequest,
    ) -> TalosResult<ProjectPatchResponse>;

    async fn delete_project(
        &self,
        actor: &Actor,
        request: ProjectDeleteRequest,
    ) -> TalosResult<ProjectDeleteResponse>;

    async fn read_fs_directory(
        &self,
        actor: &Actor,
        request: FsDirectoryReadRequest,
    ) -> TalosResult<FsDirectoryReadResponse>;

    async fn create_fs_directory(
        &self,
        actor: &Actor,
        request: FsDirectoryCreateRequest,
    ) -> TalosResult<FsDirectoryCreateResponse>;

    async fn list_models(
        &self,
        actor: &Actor,
        request: ModelListRequest,
    ) -> TalosResult<ModelListResponse>;

    async fn get_config(
        &self,
        actor: &Actor,
        request: ConfigGetRequest,
    ) -> TalosResult<ConfigResponse>;

    async fn update_config(
        &self,
        actor: &Actor,
        request: ConfigUpdateRequest,
    ) -> TalosResult<ConfigResponse>;

    async fn delete_config(
        &self,
        actor: &Actor,
        request: ConfigDeleteRequest,
    ) -> TalosResult<ConfigDeleteResponse>;

    async fn reload_config(
        &self,
        actor: &Actor,
        request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse>;

    async fn list_skills(
        &self,
        actor: &Actor,
        request: SkillsListRequest,
    ) -> TalosResult<SkillsListResponse>;

    async fn refresh_skills(
        &self,
        actor: &Actor,
        request: SkillsRefreshRequest,
    ) -> TalosResult<SkillsListResponse>;

    async fn suggest_skills(
        &self,
        actor: &Actor,
        request: SkillsSuggestRequest,
    ) -> TalosResult<SkillsSuggestResponse>;

    async fn read_skill(
        &self,
        actor: &Actor,
        request: SkillsReadRequest,
    ) -> TalosResult<SkillsReadResponse>;

    async fn resolve_skills(
        &self,
        actor: &Actor,
        request: SkillsResolveRequest,
    ) -> TalosResult<SkillsResolveResponse>;

    async fn list_plugin_sources(
        &self,
        actor: &Actor,
        request: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse>;

    async fn add_plugin_source(
        &self,
        actor: &Actor,
        request: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse>;

    async fn refresh_plugin_source(
        &self,
        actor: &Actor,
        request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse>;

    async fn remove_plugin_source(
        &self,
        actor: &Actor,
        request: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse>;

    async fn search_plugins(
        &self,
        actor: &Actor,
        request: PluginSearchRequest,
    ) -> TalosResult<PluginSearchResponse>;

    /// Reads bounded connection status for authenticated single-instance clients.
    async fn import_mcp_server(
        &self,
        actor: &Actor,
        request: talos_types::McpServerImportRequest,
    ) -> TalosResult<talos_types::PluginInstallResponse>;

    async fn start_mcp_authentication(
        &self,
        actor: &Actor,
        request: talos_types::McpAuthenticationStartRequest,
    ) -> TalosResult<talos_types::McpAuthenticationStartResponse>;

    async fn read_mcp_authentication(
        &self,
        actor: &Actor,
        request: talos_types::McpAuthenticationReadRequest,
    ) -> TalosResult<talos_types::McpAuthenticationResponse>;

    async fn update_mcp_credentials(
        &self,
        actor: &Actor,
        request: talos_types::McpCredentialsUpdateRequest,
    ) -> TalosResult<talos_types::McpAuthenticationResponse>;

    async fn list_mcp_servers(
        &self,
        actor: &Actor,
        request: talos_types::McpServerListRequest,
    ) -> TalosResult<talos_types::McpServerListResponse>;

    async fn list_plugins(
        &self,
        actor: &Actor,
        request: PluginListRequest,
    ) -> TalosResult<PluginListResponse>;

    async fn read_plugin(
        &self,
        actor: &Actor,
        request: PluginReadRequest,
    ) -> TalosResult<PluginReadResponse>;

    async fn install_plugin(
        &self,
        actor: &Actor,
        request: PluginInstallRequest,
    ) -> TalosResult<PluginInstallResponse>;

    async fn update_plugin(
        &self,
        actor: &Actor,
        request: PluginUpdateRequest,
    ) -> TalosResult<PluginUpdateResponse>;

    async fn uninstall_plugin(
        &self,
        actor: &Actor,
        request: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse>;

    async fn enable_plugin(
        &self,
        actor: &Actor,
        request: PluginEnableRequest,
    ) -> TalosResult<PluginEnableResponse>;

    async fn disable_plugin(
        &self,
        actor: &Actor,
        request: PluginDisableRequest,
    ) -> TalosResult<PluginDisableResponse>;

    /// Streams one authenticated upload into the shared file lifecycle service.
    async fn upload_file(
        &self,
        _actor: &Actor,
        _request: FileUploadRequest,
        _body: FileByteStream,
    ) -> TalosResult<FileUploadResponse> {
        Err(file_gateway_unavailable())
    }

    /// Lists files visible to one authenticated actor.
    async fn list_files(
        &self,
        _actor: &Actor,
        _request: FileListRequest,
    ) -> TalosResult<FileListResponse> {
        Err(file_gateway_unavailable())
    }

    /// Reads one actor-owned file metadata record.
    async fn read_file(
        &self,
        _actor: &Actor,
        _request: FileReadRequest,
    ) -> TalosResult<FileReadResponse> {
        Err(file_gateway_unavailable())
    }

    /// Reads actor-owned content with FileService authorization and byte limits.
    async fn read_file_content(
        &self,
        _actor: &Actor,
        _request: FileReadRequest,
    ) -> TalosResult<talos_types::FileContentResponse> {
        Err(file_gateway_unavailable())
    }

    /// Updates mutable metadata for one actor-owned file.
    async fn update_file(
        &self,
        _actor: &Actor,
        _request: FileUpdateRequest,
    ) -> TalosResult<FileUpdateResponse> {
        Err(file_gateway_unavailable())
    }

    /// Deletes one actor-owned file through its lifecycle owner.
    async fn delete_file(
        &self,
        _actor: &Actor,
        _request: FileDeleteRequest,
    ) -> TalosResult<FileDeleteResponse> {
        Err(file_gateway_unavailable())
    }

    async fn list_memory_files(
        &self,
        actor: &Actor,
        request: MemoryFileListRequest,
    ) -> TalosResult<MemoryFileListResponse>;

    async fn read_memory_file(
        &self,
        actor: &Actor,
        request: MemoryFileReadRequest,
    ) -> TalosResult<MemoryFileReadResponse>;

    async fn write_memory_file(
        &self,
        actor: &Actor,
        request: MemoryFileWriteRequest,
    ) -> TalosResult<MemoryFileWriteResponse>;

    async fn search_memory(
        &self,
        actor: &Actor,
        request: MemorySearchRequest,
    ) -> TalosResult<MemorySearchResponse>;

    async fn get_memory(
        &self,
        actor: &Actor,
        request: MemoryGetRequest,
    ) -> TalosResult<MemoryGetResponse>;

    async fn workspace_info(
        &self,
        actor: &Actor,
        request: WorkspaceInfoRequest,
    ) -> TalosResult<WorkspaceInfoResponse>;

    async fn list_workspace_docs(
        &self,
        actor: &Actor,
        request: WorkspaceDocListRequest,
    ) -> TalosResult<WorkspaceDocListResponse>;

    async fn read_workspace_doc(
        &self,
        actor: &Actor,
        request: WorkspaceDocReadRequest,
    ) -> TalosResult<WorkspaceDocReadResponse>;

    async fn write_workspace_doc(
        &self,
        actor: &Actor,
        request: WorkspaceDocWriteRequest,
    ) -> TalosResult<WorkspaceDocWriteResponse>;

    async fn logout(
        &self,
        actor: &Actor,
        request: AuthLogoutRequest,
    ) -> TalosResult<AuthLogoutResponse>;

    async fn start_cloud_login(
        &self,
        actor: &Actor,
        request: CloudLoginStartRequest,
        persistence: CloudLoginPersistence,
    ) -> TalosResult<CloudLoginStartResponse>;

    async fn cloud_login_status(
        &self,
        actor: &Actor,
        request: CloudLoginStatusRequest,
    ) -> TalosResult<CloudLoginStatusResponse>;

    async fn cancel_cloud_login(
        &self,
        actor: &Actor,
        request: CloudLoginCancelRequest,
    ) -> TalosResult<CloudLoginCancelResponse>;

    async fn read_auth_session(
        &self,
        actor: &Actor,
        request: AuthSessionReadRequest,
    ) -> TalosResult<AuthSessionReadResponse>;

    async fn clear_auth_session(
        &self,
        actor: &Actor,
        request: AuthSessionClearRequest,
    ) -> TalosResult<AuthSessionClearResponse>;

    async fn current_user(
        &self,
        actor: &Actor,
        request: UserCurrentRequest,
    ) -> TalosResult<UserCurrentResponse>;

    async fn start(&self) -> TalosResult<()>;
}

fn file_gateway_unavailable() -> TalosError {
    TalosError::new(
        talos_error::ErrorCode::ServiceUnavailable,
        "file API is not implemented by this gateway".to_string(),
    )
}
