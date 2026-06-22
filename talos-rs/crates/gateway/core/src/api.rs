use async_trait::async_trait;
use talos_application_service::Actor;
use talos_auth::{
    CloudLoginCancelRequest, CloudLoginStartRequest, CloudLoginStartResponse,
    CloudLoginStatusRequest, CloudLoginStatusResponse,
};
use talos_error::TalosResult;
use talos_types::{
    AuthLogoutRequest, AuthLogoutResponse, AuthSessionClearRequest, AuthSessionClearResponse,
    AuthSessionReadRequest, AuthSessionReadResponse, AuthSessionStoreRequest,
    AuthSessionStoreResponse, CloudLoginCancelResponse, ConfigDeleteRequest, ConfigDeleteResponse,
    ConfigGetRequest, ConfigGetResponse, ConfigReloadRequest, ConfigReloadResponse,
    ConfigUpdateRequest, ConfigUpdateResponse, FsDirectoryCreateRequest, FsDirectoryCreateResponse,
    FsDirectoryReadRequest, FsDirectoryReadResponse, InterruptTurnRequest, InterruptTurnResponse,
    MemoryFileListRequest, MemoryFileListResponse, MemoryFileReadRequest, MemoryFileReadResponse,
    MemoryFileWriteRequest, MemoryFileWriteResponse, MemorySearchRequest, MemorySearchResponse,
    ModelListRequest, ModelListResponse, PluginDisableRequest, PluginDisableResponse,
    PluginEnableRequest, PluginEnableResponse, PluginInstallRequest, PluginInstallResponse,
    PluginListRequest, PluginListResponse, PluginReadRequest, PluginReadResponse,
    PluginSearchRequest, PluginSearchResponse, PluginSourceAddRequest, PluginSourceAddResponse,
    PluginSourceListRequest, PluginSourceListResponse, PluginSourceRefreshRequest,
    PluginSourceRefreshResponse, PluginSourceRemoveRequest, PluginSourceRemoveResponse,
    PluginUninstallRequest, PluginUninstallResponse, PluginUpdateRequest, PluginUpdateResponse,
    ProjectCreateRequest, ProjectCreateResponse, ProjectDeleteRequest, ProjectDeleteResponse,
    ProjectListRequest, ProjectListResponse, ProjectPatchRequest, ProjectPatchResponse,
    ProjectReadRequest, ProjectReadResponse, RecoverTimelineRequest, RetryTurnRequest,
    RetryTurnResponse, SessionActiveTurnRequest, SessionActiveTurnResponse, SessionClearRequest,
    SessionClearResponse, SessionCloseRequest, SessionCloseResponse, SessionCompactRequest,
    SessionCompactResponse, SessionDeleteRequest, SessionDeleteResponse, SessionGetRequest,
    SessionGetResponse, SessionHistoryRequest, SessionHistoryResponse, SessionListRequest,
    SessionListResponse, SessionPatchRequest, SessionUpdateResponse, SkillsListRequest,
    SkillsListResponse, SkillsReadRequest, SkillsReadResponse, SkillsRefreshRequest,
    SkillsResolveRequest, SkillsResolveResponse, SkillsSuggestRequest, SkillsSuggestResponse,
    StartTurnRequest, StartTurnResponse, SubmitToolResultRequest, SubmitToolResultResponse,
    SubscribeTimelineRequest, SubscribeTimelineResponse, TimelineRecoveryResponse,
    UserCurrentRequest, UserCurrentResponse, WorkspaceDocListRequest, WorkspaceDocListResponse,
    WorkspaceDocReadRequest, WorkspaceDocReadResponse, WorkspaceDocWriteRequest,
    WorkspaceDocWriteResponse, WorkspaceInfoRequest, WorkspaceInfoResponse,
};

#[async_trait]
pub trait Gateway: Send + Sync {
    async fn start_turn(
        &self,
        actor: &Actor,
        request: StartTurnRequest,
        idempotency_key: Option<String>,
    ) -> TalosResult<StartTurnResponse>;

    async fn retry_turn(
        &self,
        actor: &Actor,
        request: RetryTurnRequest,
    ) -> TalosResult<RetryTurnResponse>;

    async fn interrupt_turn(
        &self,
        actor: &Actor,
        request: InterruptTurnRequest,
    ) -> TalosResult<InterruptTurnResponse>;

    async fn submit_tool_result(
        &self,
        actor: &Actor,
        request: SubmitToolResultRequest,
    ) -> TalosResult<SubmitToolResultResponse>;

    async fn subscribe_timeline(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<SubscribeTimelineResponse>;

    async fn recover_timeline(
        &self,
        actor: &Actor,
        request: RecoverTimelineRequest,
    ) -> TalosResult<TimelineRecoveryResponse>;

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
    ) -> TalosResult<ConfigGetResponse>;

    async fn update_config(
        &self,
        actor: &Actor,
        request: ConfigUpdateRequest,
    ) -> TalosResult<ConfigUpdateResponse>;

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

    async fn workspace_info(
        &self,
        actor: &Actor,
        request: WorkspaceInfoRequest,
    ) -> TalosResult<WorkspaceInfoResponse>;

    async fn logout(
        &self,
        actor: &Actor,
        request: AuthLogoutRequest,
    ) -> TalosResult<AuthLogoutResponse>;

    async fn start_cloud_login(
        &self,
        actor: &Actor,
        request: CloudLoginStartRequest,
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

    async fn store_auth_session(
        &self,
        actor: &Actor,
        request: AuthSessionStoreRequest,
    ) -> TalosResult<AuthSessionStoreResponse>;

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
