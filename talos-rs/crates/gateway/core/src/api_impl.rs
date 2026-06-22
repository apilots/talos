use async_trait::async_trait;
use talos_application_service::{Actor, TimelineSubscription};
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
    SubscribeTimelineRequest, TimelineRecoveryResponse, UserCurrentRequest, UserCurrentResponse,
    WorkspaceDocListRequest, WorkspaceDocListResponse, WorkspaceDocReadRequest,
    WorkspaceDocReadResponse, WorkspaceDocWriteRequest, WorkspaceDocWriteResponse,
    WorkspaceInfoRequest, WorkspaceInfoResponse,
};

use crate::api::Gateway;
use crate::application_service_gateway::ApplicationServiceGateway;

#[async_trait]
impl Gateway for ApplicationServiceGateway {
    async fn start_turn(
        &self,
        actor: &Actor,
        request: StartTurnRequest,
        idempotency_key: Option<String>,
    ) -> TalosResult<StartTurnResponse> {
        ApplicationServiceGateway::start_turn(self, actor, request, idempotency_key).await
    }

    async fn retry_turn(
        &self,
        actor: &Actor,
        request: RetryTurnRequest,
    ) -> TalosResult<RetryTurnResponse> {
        ApplicationServiceGateway::retry_turn(self, actor, request).await
    }

    async fn interrupt_turn(
        &self,
        actor: &Actor,
        request: InterruptTurnRequest,
    ) -> TalosResult<InterruptTurnResponse> {
        ApplicationServiceGateway::interrupt_turn(self, actor, request).await
    }

    async fn submit_tool_result(
        &self,
        actor: &Actor,
        request: SubmitToolResultRequest,
    ) -> TalosResult<SubmitToolResultResponse> {
        ApplicationServiceGateway::submit_tool_result(self, actor, request).await
    }

    async fn subscribe_timeline(
        &self,
        actor: &Actor,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineSubscription> {
        ApplicationServiceGateway::subscribe_timeline(self, actor, request).await
    }

    async fn recover_timeline(
        &self,
        actor: &Actor,
        request: RecoverTimelineRequest,
    ) -> TalosResult<TimelineRecoveryResponse> {
        ApplicationServiceGateway::recover_timeline(self, actor, request).await
    }

    async fn list_sessions(
        &self,
        actor: &Actor,
        request: SessionListRequest,
    ) -> TalosResult<SessionListResponse> {
        ApplicationServiceGateway::list_sessions(self, actor, request).await
    }

    async fn get_session(
        &self,
        actor: &Actor,
        request: SessionGetRequest,
    ) -> TalosResult<SessionGetResponse> {
        ApplicationServiceGateway::get_session(self, actor, request).await
    }

    async fn get_session_history(
        &self,
        actor: &Actor,
        request: SessionHistoryRequest,
    ) -> TalosResult<SessionHistoryResponse> {
        ApplicationServiceGateway::get_session_history(self, actor, request).await
    }

    async fn get_active_turn(
        &self,
        actor: &Actor,
        request: SessionActiveTurnRequest,
    ) -> TalosResult<SessionActiveTurnResponse> {
        ApplicationServiceGateway::get_active_turn(self, actor, request).await
    }

    async fn patch_session(
        &self,
        actor: &Actor,
        request: SessionPatchRequest,
    ) -> TalosResult<SessionUpdateResponse> {
        ApplicationServiceGateway::patch_session(self, actor, request).await
    }

    async fn close_session(
        &self,
        actor: &Actor,
        request: SessionCloseRequest,
    ) -> TalosResult<SessionCloseResponse> {
        ApplicationServiceGateway::close_session(self, actor, request).await
    }

    async fn delete_session(
        &self,
        actor: &Actor,
        request: SessionDeleteRequest,
    ) -> TalosResult<SessionDeleteResponse> {
        ApplicationServiceGateway::delete_session(self, actor, request).await
    }

    async fn compact_session(
        &self,
        actor: &Actor,
        request: SessionCompactRequest,
    ) -> TalosResult<SessionCompactResponse> {
        ApplicationServiceGateway::compact_session(self, actor, request).await
    }

    async fn clear_session(
        &self,
        actor: &Actor,
        request: SessionClearRequest,
    ) -> TalosResult<SessionClearResponse> {
        ApplicationServiceGateway::clear_session(self, actor, request).await
    }

    async fn list_projects(
        &self,
        actor: &Actor,
        request: ProjectListRequest,
    ) -> TalosResult<ProjectListResponse> {
        ApplicationServiceGateway::list_projects(self, actor, request).await
    }

    async fn get_project(
        &self,
        actor: &Actor,
        request: ProjectReadRequest,
    ) -> TalosResult<ProjectReadResponse> {
        ApplicationServiceGateway::get_project(self, actor, request).await
    }

    async fn create_project(
        &self,
        actor: &Actor,
        request: ProjectCreateRequest,
    ) -> TalosResult<ProjectCreateResponse> {
        ApplicationServiceGateway::create_project(self, actor, request).await
    }

    async fn patch_project(
        &self,
        actor: &Actor,
        request: ProjectPatchRequest,
    ) -> TalosResult<ProjectPatchResponse> {
        ApplicationServiceGateway::patch_project(self, actor, request).await
    }

    async fn delete_project(
        &self,
        actor: &Actor,
        request: ProjectDeleteRequest,
    ) -> TalosResult<ProjectDeleteResponse> {
        ApplicationServiceGateway::delete_project(self, actor, request).await
    }

    async fn read_fs_directory(
        &self,
        actor: &Actor,
        request: FsDirectoryReadRequest,
    ) -> TalosResult<FsDirectoryReadResponse> {
        ApplicationServiceGateway::read_fs_directory(self, actor, request).await
    }

    async fn create_fs_directory(
        &self,
        actor: &Actor,
        request: FsDirectoryCreateRequest,
    ) -> TalosResult<FsDirectoryCreateResponse> {
        ApplicationServiceGateway::create_fs_directory(self, actor, request).await
    }

    async fn list_models(
        &self,
        actor: &Actor,
        request: ModelListRequest,
    ) -> TalosResult<ModelListResponse> {
        ApplicationServiceGateway::list_models(self, actor, request).await
    }

    async fn get_config(
        &self,
        actor: &Actor,
        request: ConfigGetRequest,
    ) -> TalosResult<ConfigGetResponse> {
        ApplicationServiceGateway::get_config(self, actor, request).await
    }

    async fn update_config(
        &self,
        actor: &Actor,
        request: ConfigUpdateRequest,
    ) -> TalosResult<ConfigUpdateResponse> {
        ApplicationServiceGateway::update_config(self, actor, request).await
    }

    async fn delete_config(
        &self,
        actor: &Actor,
        request: ConfigDeleteRequest,
    ) -> TalosResult<ConfigDeleteResponse> {
        ApplicationServiceGateway::delete_config(self, actor, request).await
    }

    async fn reload_config(
        &self,
        actor: &Actor,
        request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse> {
        ApplicationServiceGateway::reload_config(self, actor, request).await
    }

    async fn list_skills(
        &self,
        actor: &Actor,
        request: SkillsListRequest,
    ) -> TalosResult<SkillsListResponse> {
        ApplicationServiceGateway::list_skills(self, actor, request).await
    }

    async fn refresh_skills(
        &self,
        actor: &Actor,
        request: SkillsRefreshRequest,
    ) -> TalosResult<SkillsListResponse> {
        ApplicationServiceGateway::refresh_skills(self, actor, request).await
    }

    async fn suggest_skills(
        &self,
        actor: &Actor,
        request: SkillsSuggestRequest,
    ) -> TalosResult<SkillsSuggestResponse> {
        ApplicationServiceGateway::suggest_skills(self, actor, request).await
    }

    async fn read_skill(
        &self,
        actor: &Actor,
        request: SkillsReadRequest,
    ) -> TalosResult<SkillsReadResponse> {
        ApplicationServiceGateway::read_skill(self, actor, request).await
    }

    async fn resolve_skills(
        &self,
        actor: &Actor,
        request: SkillsResolveRequest,
    ) -> TalosResult<SkillsResolveResponse> {
        ApplicationServiceGateway::resolve_skills(self, actor, request).await
    }

    async fn list_plugin_sources(
        &self,
        actor: &Actor,
        request: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse> {
        ApplicationServiceGateway::list_plugin_sources(self, actor, request).await
    }

    async fn add_plugin_source(
        &self,
        actor: &Actor,
        request: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse> {
        ApplicationServiceGateway::add_plugin_source(self, actor, request).await
    }

    async fn refresh_plugin_source(
        &self,
        actor: &Actor,
        request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        ApplicationServiceGateway::refresh_plugin_source(self, actor, request).await
    }

    async fn remove_plugin_source(
        &self,
        actor: &Actor,
        request: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse> {
        ApplicationServiceGateway::remove_plugin_source(self, actor, request).await
    }

    async fn search_plugins(
        &self,
        actor: &Actor,
        request: PluginSearchRequest,
    ) -> TalosResult<PluginSearchResponse> {
        ApplicationServiceGateway::search_plugins(self, actor, request).await
    }

    async fn list_plugins(
        &self,
        actor: &Actor,
        request: PluginListRequest,
    ) -> TalosResult<PluginListResponse> {
        ApplicationServiceGateway::list_plugins(self, actor, request).await
    }

    async fn read_plugin(
        &self,
        actor: &Actor,
        request: PluginReadRequest,
    ) -> TalosResult<PluginReadResponse> {
        ApplicationServiceGateway::read_plugin(self, actor, request).await
    }

    async fn install_plugin(
        &self,
        actor: &Actor,
        request: PluginInstallRequest,
    ) -> TalosResult<PluginInstallResponse> {
        ApplicationServiceGateway::install_plugin(self, actor, request).await
    }

    async fn update_plugin(
        &self,
        actor: &Actor,
        request: PluginUpdateRequest,
    ) -> TalosResult<PluginUpdateResponse> {
        ApplicationServiceGateway::update_plugin(self, actor, request).await
    }

    async fn uninstall_plugin(
        &self,
        actor: &Actor,
        request: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse> {
        ApplicationServiceGateway::uninstall_plugin(self, actor, request).await
    }

    async fn enable_plugin(
        &self,
        actor: &Actor,
        request: PluginEnableRequest,
    ) -> TalosResult<PluginEnableResponse> {
        ApplicationServiceGateway::enable_plugin(self, actor, request).await
    }

    async fn disable_plugin(
        &self,
        actor: &Actor,
        request: PluginDisableRequest,
    ) -> TalosResult<PluginDisableResponse> {
        ApplicationServiceGateway::disable_plugin(self, actor, request).await
    }

    async fn list_memory_files(
        &self,
        actor: &Actor,
        request: MemoryFileListRequest,
    ) -> TalosResult<MemoryFileListResponse> {
        ApplicationServiceGateway::list_memory_files(self, actor, request).await
    }

    async fn read_memory_file(
        &self,
        actor: &Actor,
        request: MemoryFileReadRequest,
    ) -> TalosResult<MemoryFileReadResponse> {
        ApplicationServiceGateway::read_memory_file(self, actor, request).await
    }

    async fn write_memory_file(
        &self,
        actor: &Actor,
        request: MemoryFileWriteRequest,
    ) -> TalosResult<MemoryFileWriteResponse> {
        ApplicationServiceGateway::write_memory_file(self, actor, request).await
    }

    async fn search_memory(
        &self,
        actor: &Actor,
        request: MemorySearchRequest,
    ) -> TalosResult<MemorySearchResponse> {
        ApplicationServiceGateway::search_memory(self, actor, request).await
    }

    async fn list_workspace_docs(
        &self,
        actor: &Actor,
        request: WorkspaceDocListRequest,
    ) -> TalosResult<WorkspaceDocListResponse> {
        ApplicationServiceGateway::list_workspace_docs(self, actor, request).await
    }

    async fn read_workspace_doc(
        &self,
        actor: &Actor,
        request: WorkspaceDocReadRequest,
    ) -> TalosResult<WorkspaceDocReadResponse> {
        ApplicationServiceGateway::read_workspace_doc(self, actor, request).await
    }

    async fn write_workspace_doc(
        &self,
        actor: &Actor,
        request: WorkspaceDocWriteRequest,
    ) -> TalosResult<WorkspaceDocWriteResponse> {
        ApplicationServiceGateway::write_workspace_doc(self, actor, request).await
    }

    async fn workspace_info(
        &self,
        actor: &Actor,
        request: WorkspaceInfoRequest,
    ) -> TalosResult<WorkspaceInfoResponse> {
        ApplicationServiceGateway::workspace_info(self, actor, request).await
    }

    async fn logout(
        &self,
        actor: &Actor,
        request: AuthLogoutRequest,
    ) -> TalosResult<AuthLogoutResponse> {
        ApplicationServiceGateway::logout(self, actor, request).await
    }

    async fn start_cloud_login(
        &self,
        actor: &Actor,
        request: CloudLoginStartRequest,
    ) -> TalosResult<CloudLoginStartResponse> {
        ApplicationServiceGateway::start_cloud_login(self, actor, request).await
    }

    async fn cloud_login_status(
        &self,
        actor: &Actor,
        request: CloudLoginStatusRequest,
    ) -> TalosResult<CloudLoginStatusResponse> {
        ApplicationServiceGateway::cloud_login_status(self, actor, request).await
    }

    async fn cancel_cloud_login(
        &self,
        actor: &Actor,
        request: CloudLoginCancelRequest,
    ) -> TalosResult<CloudLoginCancelResponse> {
        ApplicationServiceGateway::cancel_cloud_login(self, actor, request).await
    }

    async fn read_auth_session(
        &self,
        actor: &Actor,
        request: AuthSessionReadRequest,
    ) -> TalosResult<AuthSessionReadResponse> {
        ApplicationServiceGateway::read_auth_session(self, actor, request).await
    }

    async fn store_auth_session(
        &self,
        actor: &Actor,
        request: AuthSessionStoreRequest,
    ) -> TalosResult<AuthSessionStoreResponse> {
        ApplicationServiceGateway::store_auth_session(self, actor, request).await
    }

    async fn clear_auth_session(
        &self,
        actor: &Actor,
        request: AuthSessionClearRequest,
    ) -> TalosResult<AuthSessionClearResponse> {
        ApplicationServiceGateway::clear_auth_session(self, actor, request).await
    }

    async fn current_user(
        &self,
        actor: &Actor,
        request: UserCurrentRequest,
    ) -> TalosResult<UserCurrentResponse> {
        ApplicationServiceGateway::current_user(self, actor, request).await
    }

    async fn start(&self) -> TalosResult<()> {
        ApplicationServiceGateway::start(self).await
    }
}
