use async_trait::async_trait;
use bytes::Bytes;
use talos_agent_api::{
    InterruptTurnRequest, InterruptTurnResponse, RecoverTimelineResponse,
    SessionActiveTurnResponse, SessionHistoryResponse, SubmitTurnInputRequest,
    SubmitTurnInputResponse, TimelineEvent,
};
use talos_auth::{
    CloudLoginCancelRequest, CloudLoginStartRequest, CloudLoginStartResponse,
    CloudLoginStatusRequest, CloudLoginStatusResponse,
};
use talos_error::{TalosError, TalosResult};
use talos_types::{
    AuthLogoutRequest, AuthLogoutResponse, AuthSessionClearRequest, AuthSessionClearResponse,
    AuthSessionReadRequest, AuthSessionReadResponse, CloudLoginCancelResponse, ConfigDeleteRequest,
    ConfigDeleteResponse, ConfigGetRequest, ConfigReloadRequest, ConfigReloadResponse,
    ConfigResponse, ConfigUpdateRequest, EventUnsubscribeRequest, EventUnsubscribeResponse,
    FileDeleteRequest, FileDeleteResponse, FileListRequest, FileListResponse, FileReadRequest,
    FileReadResponse, FileUpdateRequest, FileUpdateResponse, FileUploadRequest, FileUploadResponse,
    FsDirectoryCreateRequest, FsDirectoryCreateResponse, FsDirectoryReadRequest,
    FsDirectoryReadResponse, MemoryFileListRequest, MemoryFileListResponse, MemoryFileReadRequest,
    MemoryFileReadResponse, MemoryFileWriteRequest, MemoryFileWriteResponse, MemoryGetRequest,
    MemoryGetResponse, MemorySearchRequest, MemorySearchResponse, MessageQueue,
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
use tokio::sync::mpsc;

pub struct TimelineEventStream {
    receiver: mpsc::Receiver<TimelineEvent>,
}

pub struct TimelineStreamSubscription {
    pub response: SubscribeTimelineResponse,
    pub stream: TimelineEventStream,
    pub message_queue_changes: MessageQueueChangeStream,
}

/// Live queue invalidations delivered by an existing Session subscription.
///
/// Notifications are not replayed. Callers must list the complete queue after
/// connecting and after observing a newer revision.
pub struct MessageQueueChangeStream {
    receiver: mpsc::Receiver<talos_types::MessageQueueChangedNotification>,
}

impl TimelineEventStream {
    pub fn new(receiver: mpsc::Receiver<TimelineEvent>) -> Self {
        Self { receiver }
    }

    pub async fn recv(&mut self) -> TalosResult<TimelineEvent> {
        self.receiver
            .recv()
            .await
            .ok_or_else(|| TalosError::internal("timeline event stream closed"))
    }

    pub fn into_inner(self) -> mpsc::Receiver<TimelineEvent> {
        self.receiver
    }
}

impl MessageQueueChangeStream {
    pub fn new(receiver: mpsc::Receiver<talos_types::MessageQueueChangedNotification>) -> Self {
        Self { receiver }
    }

    pub async fn recv(&mut self) -> Option<talos_types::MessageQueueChangedNotification> {
        self.receiver.recv().await
    }
}

#[async_trait]
pub trait GatewayClient: Send {
    /// Reads current-account Sync observations without enabling sync or exposing credentials.
    async fn sync_metrics(&mut self) -> TalosResult<talos_types::SyncMetricsResponse>;

    /// Lists caller-owned Computers and the shared embedded Computer, including offline metadata.
    async fn list_computers(
        &mut self,
        request: talos_types::ComputerListRequest,
    ) -> TalosResult<talos_types::ComputerListResponse>;

    /// Reads an authorized Computer without exposing registration or lease credentials.
    async fn read_computer(
        &mut self,
        request: talos_types::GetNodeRequest,
    ) -> TalosResult<talos_types::NodeInfo>;

    /// Creates a caller-owned UUID and returns its one-time credential; protect the token at the host boundary.
    async fn create_computer(
        &mut self,
        request: talos_types::CreateNodeRequest,
    ) -> TalosResult<talos_types::CreatedNode>;

    /// Updates owner-visible metadata while preserving the UUID and Session bindings.
    async fn rename_computer(
        &mut self,
        request: talos_types::RenameComputerRequest,
    ) -> TalosResult<talos_types::NodeInfo>;

    /// Changes registration availability; disabling revokes the active lease and execution handle.
    /// Removes an owned registration after idle verification, retaining history and files.
    async fn delete_computer(
        &mut self,
        request: talos_types::DeleteComputerRequest,
    ) -> TalosResult<talos_types::DeleteComputerResponse>;

    async fn set_computer_status(
        &mut self,
        request: talos_types::SetComputerStatusRequest,
    ) -> TalosResult<talos_types::NodeInfo>;

    /// Revokes existing credentials and returns an owner-authorized one-time replacement.
    async fn rotate_computer_credential(
        &mut self,
        request: talos_types::RotateCredentialRequest,
    ) -> TalosResult<talos_types::NodeCredential>;

    async fn read_auth_session(
        &mut self,
        request: AuthSessionReadRequest,
    ) -> TalosResult<AuthSessionReadResponse>;

    async fn clear_auth_session(
        &mut self,
        request: AuthSessionClearRequest,
    ) -> TalosResult<AuthSessionClearResponse>;

    async fn auth_logout(&mut self, request: AuthLogoutRequest) -> TalosResult<AuthLogoutResponse>;

    async fn start_cloud_login(
        &mut self,
        request: CloudLoginStartRequest,
    ) -> TalosResult<CloudLoginStartResponse>;

    async fn cloud_login_status(
        &mut self,
        request: CloudLoginStatusRequest,
    ) -> TalosResult<CloudLoginStatusResponse>;

    async fn cancel_cloud_login(
        &mut self,
        request: CloudLoginCancelRequest,
    ) -> TalosResult<CloudLoginCancelResponse>;

    async fn current_user(
        &mut self,
        request: UserCurrentRequest,
    ) -> TalosResult<UserCurrentResponse>;

    /// Reads the authenticated caller's Profile usage summary.
    async fn read_profile(
        &mut self,
        request: ProfileReadRequest,
    ) -> TalosResult<ProfileReadResponse>;

    async fn list_models(&mut self, request: ModelListRequest) -> TalosResult<ModelListResponse>;

    async fn get_config(&mut self, request: ConfigGetRequest) -> TalosResult<ConfigResponse>;

    async fn update_config(&mut self, request: ConfigUpdateRequest) -> TalosResult<ConfigResponse>;

    async fn delete_config(
        &mut self,
        request: ConfigDeleteRequest,
    ) -> TalosResult<ConfigDeleteResponse>;

    async fn reload_config(
        &mut self,
        request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse>;

    async fn list_skills(&mut self, request: SkillsListRequest) -> TalosResult<SkillsListResponse>;

    async fn refresh_skills(
        &mut self,
        request: SkillsRefreshRequest,
    ) -> TalosResult<SkillsListResponse>;

    async fn suggest_skills(
        &mut self,
        request: SkillsSuggestRequest,
    ) -> TalosResult<SkillsSuggestResponse>;

    async fn read_skill(&mut self, request: SkillsReadRequest) -> TalosResult<SkillsReadResponse>;

    async fn resolve_skills(
        &mut self,
        request: SkillsResolveRequest,
    ) -> TalosResult<SkillsResolveResponse>;

    async fn list_sessions(
        &mut self,
        request: SessionListRequest,
    ) -> TalosResult<SessionListResponse>;

    async fn get_session(&mut self, request: SessionGetRequest) -> TalosResult<SessionGetResponse>;

    async fn get_session_history(
        &mut self,
        request: SessionHistoryRequest,
    ) -> TalosResult<SessionHistoryResponse>;

    async fn get_active_turn(
        &mut self,
        request: SessionActiveTurnRequest,
    ) -> TalosResult<SessionActiveTurnResponse>;

    async fn patch_session(
        &mut self,
        request: SessionPatchRequest,
    ) -> TalosResult<SessionUpdateResponse>;

    async fn close_session(
        &mut self,
        request: SessionCloseRequest,
    ) -> TalosResult<SessionCloseResponse>;

    async fn delete_session(
        &mut self,
        request: SessionDeleteRequest,
    ) -> TalosResult<SessionDeleteResponse>;

    async fn compact_session(
        &mut self,
        request: SessionCompactRequest,
    ) -> TalosResult<SessionCompactResponse>;

    async fn clear_session(
        &mut self,
        request: SessionClearRequest,
    ) -> TalosResult<SessionClearResponse>;

    async fn recover_timeline(
        &mut self,
        request: RecoverTimelineRequest,
    ) -> TalosResult<RecoverTimelineResponse>;

    async fn subscribe_timeline(
        &mut self,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<SubscribeTimelineResponse>;

    async fn open_timeline_stream(
        &mut self,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineStreamSubscription>;

    async fn unsubscribe_timeline(
        &mut self,
        request: EventUnsubscribeRequest,
    ) -> TalosResult<EventUnsubscribeResponse>;

    async fn start_turn(&mut self, request: StartTurnRequest) -> TalosResult<StartTurnResponse>;

    async fn retry_turn(&mut self, request: RetryTurnRequest) -> TalosResult<StartTurnResponse>;

    async fn interrupt_turn(
        &mut self,
        request: InterruptTurnRequest,
    ) -> TalosResult<InterruptTurnResponse>;

    /// Adds one message to the server-owned Session queue.
    async fn add_queued_message(
        &mut self,
        request: MessageQueueAddRequest,
    ) -> TalosResult<MessageQueue>;

    /// Reads the complete server-owned Session queue.
    async fn list_message_queue(
        &mut self,
        request: MessageQueueListRequest,
    ) -> TalosResult<MessageQueue>;

    /// Replaces one waiting message without changing its identity or position.
    async fn update_queued_message(
        &mut self,
        request: MessageQueueUpdateRequest,
    ) -> TalosResult<MessageQueue>;

    /// Deletes one waiting message by server-assigned ID.
    async fn delete_queued_message(
        &mut self,
        request: MessageQueueDeleteRequest,
    ) -> TalosResult<MessageQueue>;

    /// Applies a complete ordered permutation of current message IDs.
    async fn reorder_message_queue(
        &mut self,
        request: MessageQueueReorderRequest,
    ) -> TalosResult<MessageQueue>;

    /// Starts the queue head or one explicitly selected waiting message.
    async fn start_queued_message(
        &mut self,
        request: MessageQueueStartRequest,
    ) -> TalosResult<StartTurnResponse>;

    /// Delivers typed input to one exact running or waiting Turn.
    async fn submit_turn_input(
        &mut self,
        request: SubmitTurnInputRequest,
    ) -> TalosResult<SubmitTurnInputResponse>;

    async fn list_projects(
        &mut self,
        request: ProjectListRequest,
    ) -> TalosResult<ProjectListResponse>;

    async fn get_project(
        &mut self,
        request: ProjectReadRequest,
    ) -> TalosResult<ProjectReadResponse>;

    async fn create_project(
        &mut self,
        request: ProjectCreateRequest,
    ) -> TalosResult<ProjectCreateResponse>;

    async fn patch_project(
        &mut self,
        request: ProjectPatchRequest,
    ) -> TalosResult<ProjectPatchResponse>;

    async fn delete_project(
        &mut self,
        request: ProjectDeleteRequest,
    ) -> TalosResult<ProjectDeleteResponse>;

    async fn read_fs_directory(
        &mut self,
        request: FsDirectoryReadRequest,
    ) -> TalosResult<FsDirectoryReadResponse>;

    async fn create_fs_directory(
        &mut self,
        request: FsDirectoryCreateRequest,
    ) -> TalosResult<FsDirectoryCreateResponse>;

    async fn list_plugin_sources(
        &mut self,
        request: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse>;

    async fn add_plugin_source(
        &mut self,
        request: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse>;

    async fn refresh_plugin_source(
        &mut self,
        request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse>;

    async fn remove_plugin_source(
        &mut self,
        request: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse>;

    async fn search_plugins(
        &mut self,
        request: PluginSearchRequest,
    ) -> TalosResult<PluginSearchResponse>;

    async fn import_mcp_server(
        &mut self,
        request: talos_types::McpServerImportRequest,
    ) -> TalosResult<talos_types::PluginInstallResponse>;

    async fn start_mcp_authentication(
        &mut self,
        request: talos_types::McpAuthenticationStartRequest,
    ) -> TalosResult<talos_types::McpAuthenticationStartResponse>;

    async fn read_mcp_authentication(
        &mut self,
        request: talos_types::McpAuthenticationReadRequest,
    ) -> TalosResult<talos_types::McpAuthenticationResponse>;

    async fn update_mcp_credentials(
        &mut self,
        request: talos_types::McpCredentialsUpdateRequest,
    ) -> TalosResult<talos_types::McpAuthenticationResponse>;

    async fn list_mcp_servers(
        &mut self,
        request: talos_types::McpServerListRequest,
    ) -> TalosResult<talos_types::McpServerListResponse>;

    async fn list_plugins(&mut self, request: PluginListRequest)
        -> TalosResult<PluginListResponse>;

    async fn read_plugin(&mut self, request: PluginReadRequest) -> TalosResult<PluginReadResponse>;

    async fn install_plugin(
        &mut self,
        request: PluginInstallRequest,
    ) -> TalosResult<PluginInstallResponse>;

    async fn update_plugin(
        &mut self,
        request: PluginUpdateRequest,
    ) -> TalosResult<PluginUpdateResponse>;

    async fn uninstall_plugin(
        &mut self,
        request: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse>;

    async fn enable_plugin(
        &mut self,
        request: PluginEnableRequest,
    ) -> TalosResult<PluginEnableResponse>;

    async fn disable_plugin(
        &mut self,
        request: PluginDisableRequest,
    ) -> TalosResult<PluginDisableResponse>;

    /// Uploads bytes through the App Server file transport envelope.
    async fn upload_file(
        &mut self,
        _request: FileUploadRequest,
        _data: Bytes,
    ) -> TalosResult<FileUploadResponse> {
        Err(file_client_unavailable())
    }

    /// Lists files available through this client transport.
    async fn list_files(&mut self, _request: FileListRequest) -> TalosResult<FileListResponse> {
        Err(file_client_unavailable())
    }

    /// Reads one file metadata record through this client transport.
    async fn read_file(&mut self, _request: FileReadRequest) -> TalosResult<FileReadResponse> {
        Err(file_client_unavailable())
    }

    /// Reads authorized bytes without exposing the wire base64 encoding.
    async fn read_file_content(
        &mut self,
        _request: FileReadRequest,
    ) -> TalosResult<crate::FileDownloadResponse> {
        Err(file_client_unavailable())
    }

    /// Updates mutable metadata through this client transport.
    async fn update_file(
        &mut self,
        _request: FileUpdateRequest,
    ) -> TalosResult<FileUpdateResponse> {
        Err(file_client_unavailable())
    }

    /// Deletes one file through this client transport.
    async fn delete_file(
        &mut self,
        _request: FileDeleteRequest,
    ) -> TalosResult<FileDeleteResponse> {
        Err(file_client_unavailable())
    }

    async fn list_memory_files(
        &mut self,
        request: MemoryFileListRequest,
    ) -> TalosResult<MemoryFileListResponse>;

    async fn read_memory_file(
        &mut self,
        request: MemoryFileReadRequest,
    ) -> TalosResult<MemoryFileReadResponse>;

    async fn write_memory_file(
        &mut self,
        request: MemoryFileWriteRequest,
    ) -> TalosResult<MemoryFileWriteResponse>;

    async fn search_memory(
        &mut self,
        request: MemorySearchRequest,
    ) -> TalosResult<MemorySearchResponse>;

    async fn get_memory(&mut self, request: MemoryGetRequest) -> TalosResult<MemoryGetResponse>;

    async fn workspace_info(
        &mut self,
        request: WorkspaceInfoRequest,
    ) -> TalosResult<WorkspaceInfoResponse>;

    async fn list_workspace_docs(
        &mut self,
        request: WorkspaceDocListRequest,
    ) -> TalosResult<WorkspaceDocListResponse>;

    async fn read_workspace_doc(
        &mut self,
        request: WorkspaceDocReadRequest,
    ) -> TalosResult<WorkspaceDocReadResponse>;

    async fn write_workspace_doc(
        &mut self,
        request: WorkspaceDocWriteRequest,
    ) -> TalosResult<WorkspaceDocWriteResponse>;

    async fn shutdown(&mut self) -> TalosResult<()>;
}

fn file_client_unavailable() -> TalosError {
    TalosError::internal("file API is not implemented by this gateway client")
}
