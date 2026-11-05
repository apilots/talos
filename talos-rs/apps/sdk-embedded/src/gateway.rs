//! Existing product API delegation for this client transport.
use super::*;

mod timeline;
mod transport;

#[async_trait]
impl GatewayClient for EmbeddedServerClient {
    async fn session_automation(
        &mut self,
        request: talos_types::SessionAutomationReadRequest,
    ) -> TalosResult<talos_types::SessionAutomationSnapshot> {
        self.gateway()
            .session_automation(&self.actor, request)
            .await
    }
    async fn execute_automation(
        &mut self,
        request: talos_types::AutomationRequest,
    ) -> TalosResult<talos_types::AutomationControlResult> {
        self.gateway()
            .execute_automation(&self.actor, request)
            .await
    }
    async fn list_automation(
        &mut self,
        request: talos_types::AutomationListRequest,
    ) -> TalosResult<talos_types::AutomationListResponse> {
        self.gateway().list_automation(&self.actor, request).await
    }
    async fn automation_command_catalog(
        &mut self,
        request: talos_types::AutomationCommandCatalogRequest,
    ) -> TalosResult<talos_types::AutomationCommandCatalogResponse> {
        self.gateway()
            .automation_command_catalog(&self.actor, request)
            .await
    }
    async fn resolve_automation_command(
        &mut self,
        request: talos_types::AutomationCommandResolveRequest,
    ) -> TalosResult<talos_types::AutomationCommandResolveResponse> {
        self.gateway()
            .resolve_automation_command(&self.actor, request)
            .await
    }
    async fn list_builtin_jobs(
        &mut self,
        request: talos_types::BuiltinJobListRequest,
    ) -> TalosResult<talos_types::BuiltinJobListResponse> {
        self.gateway().list_builtin_jobs(&self.actor, request).await
    }
    async fn control_builtin_job(
        &mut self,
        request: talos_types::BuiltinJobControlRequest,
    ) -> TalosResult<talos_types::BuiltinJobInfo> {
        self.gateway()
            .control_builtin_job(&self.actor, request)
            .await
    }

    async fn sync_metrics(&mut self) -> TalosResult<talos_types::SyncMetricsResponse> {
        self.runtime.service.sync_metrics(&self.actor).await
    }

    async fn list_computers(
        &mut self,
        request: talos_types::ComputerListRequest,
    ) -> TalosResult<talos_types::ComputerListResponse> {
        self.gateway().list_computers(&self.actor, request).await
    }

    async fn read_computer(
        &mut self,
        request: talos_types::GetNodeRequest,
    ) -> TalosResult<talos_types::NodeInfo> {
        self.gateway().read_computer(&self.actor, request).await
    }

    async fn create_computer(
        &mut self,
        request: talos_types::CreateNodeRequest,
    ) -> TalosResult<talos_types::CreatedNode> {
        self.gateway().create_computer(&self.actor, request).await
    }

    async fn rename_computer(
        &mut self,
        request: talos_types::RenameComputerRequest,
    ) -> TalosResult<talos_types::NodeInfo> {
        self.gateway().rename_computer(&self.actor, request).await
    }

    async fn delete_computer(
        &mut self,
        request: talos_types::DeleteComputerRequest,
    ) -> TalosResult<talos_types::DeleteComputerResponse> {
        self.gateway().delete_computer(&self.actor, request).await
    }

    async fn set_computer_status(
        &mut self,
        request: talos_types::SetComputerStatusRequest,
    ) -> TalosResult<talos_types::NodeInfo> {
        self.gateway()
            .set_computer_status(&self.actor, request)
            .await
    }

    async fn rotate_computer_credential(
        &mut self,
        request: talos_types::RotateCredentialRequest,
    ) -> TalosResult<talos_types::NodeCredential> {
        self.gateway()
            .rotate_computer_credential(&self.actor, request)
            .await
    }

    async fn import_mcp_server(
        &mut self,
        request: talos_sdk::McpServerImportRequest,
    ) -> TalosResult<talos_sdk::PluginInstallResponse> {
        self.gateway().import_mcp_server(&self.actor, request).await
    }

    async fn start_mcp_authentication(
        &mut self,
        request: talos_sdk::McpAuthenticationStartRequest,
    ) -> TalosResult<talos_sdk::McpAuthenticationStartResponse> {
        self.gateway()
            .start_mcp_authentication(&self.actor, request)
            .await
    }

    async fn read_mcp_authentication(
        &mut self,
        request: talos_sdk::McpAuthenticationReadRequest,
    ) -> TalosResult<talos_sdk::McpAuthenticationResponse> {
        self.gateway()
            .read_mcp_authentication(&self.actor, request)
            .await
    }

    async fn update_mcp_credentials(
        &mut self,
        request: talos_sdk::McpCredentialsUpdateRequest,
    ) -> TalosResult<talos_sdk::McpAuthenticationResponse> {
        self.gateway()
            .update_mcp_credentials(&self.actor, request)
            .await
    }

    async fn open_desktop(
        &mut self,
        request: talos_sdk::DesktopOpenRequest,
    ) -> TalosResult<talos_sdk::DesktopViewGrant> {
        self.gateway().open_desktop(&self.actor, request).await
    }

    async fn browser(
        &mut self,
        request: talos_sdk::BrowserRequest,
    ) -> TalosResult<talos_sdk::BrowserSnapshot> {
        self.gateway().browser(&self.actor, request).await
    }

    async fn list_mcp_servers(
        &mut self,
        request: talos_sdk::McpServerListRequest,
    ) -> TalosResult<talos_sdk::McpServerListResponse> {
        self.gateway().list_mcp_servers(&self.actor, request).await
    }

    async fn read_auth_session(
        &mut self,
        request: AuthSessionReadRequest,
    ) -> TalosResult<AuthSessionReadResponse> {
        self.gateway().read_auth_session(&self.actor, request).await
    }

    async fn clear_auth_session(
        &mut self,
        request: AuthSessionClearRequest,
    ) -> TalosResult<AuthSessionClearResponse> {
        self.gateway()
            .clear_auth_session(&self.actor, request)
            .await
    }

    async fn auth_logout(&mut self, request: AuthLogoutRequest) -> TalosResult<AuthLogoutResponse> {
        self.gateway().logout(&self.actor, request).await
    }

    async fn start_cloud_login(
        &mut self,
        request: CloudLoginStartRequest,
    ) -> TalosResult<CloudLoginStartResponse> {
        self.gateway()
            .start_cloud_login(&self.actor, request, CloudLoginPersistence::LocalCache)
            .await
    }

    async fn cloud_login_status(
        &mut self,
        request: CloudLoginStatusRequest,
    ) -> TalosResult<CloudLoginStatusResponse> {
        self.refresh_embedded_login(request).await
    }

    async fn cancel_cloud_login(
        &mut self,
        request: CloudLoginCancelRequest,
    ) -> TalosResult<CloudLoginCancelResponse> {
        self.gateway()
            .cancel_cloud_login(&self.actor, request)
            .await
    }

    async fn current_user(
        &mut self,
        request: UserCurrentRequest,
    ) -> TalosResult<UserCurrentResponse> {
        self.gateway().current_user(&self.actor, request).await
    }

    async fn read_profile(
        &mut self,
        request: ProfileReadRequest,
    ) -> TalosResult<ProfileReadResponse> {
        self.gateway().read_profile(&self.actor, request).await
    }

    async fn list_models(&mut self, request: ModelListRequest) -> TalosResult<ModelListResponse> {
        self.gateway().list_models(&self.actor, request).await
    }

    async fn get_config(&mut self, request: ConfigGetRequest) -> TalosResult<ConfigResponse> {
        self.gateway().get_config(&self.actor, request).await
    }

    async fn update_config(&mut self, request: ConfigUpdateRequest) -> TalosResult<ConfigResponse> {
        self.gateway().update_config(&self.actor, request).await
    }

    async fn delete_config(
        &mut self,
        request: ConfigDeleteRequest,
    ) -> TalosResult<ConfigDeleteResponse> {
        self.gateway().delete_config(&self.actor, request).await
    }

    async fn reload_config(
        &mut self,
        request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse> {
        self.gateway().reload_config(&self.actor, request).await
    }

    async fn list_skills(
        &mut self,
        request: talos_sdk::SkillsListRequest,
    ) -> TalosResult<talos_sdk::SkillsListResponse> {
        self.gateway().list_skills(&self.actor, request).await
    }

    async fn refresh_skills(
        &mut self,
        request: talos_sdk::SkillsRefreshRequest,
    ) -> TalosResult<talos_sdk::SkillsListResponse> {
        self.gateway().refresh_skills(&self.actor, request).await
    }

    async fn suggest_skills(
        &mut self,
        request: talos_sdk::SkillsSuggestRequest,
    ) -> TalosResult<talos_sdk::SkillsSuggestResponse> {
        self.gateway().suggest_skills(&self.actor, request).await
    }

    async fn read_skill(
        &mut self,
        request: talos_sdk::SkillsReadRequest,
    ) -> TalosResult<SkillsReadResponse> {
        self.gateway().read_skill(&self.actor, request).await
    }

    async fn resolve_skills(
        &mut self,
        request: talos_sdk::SkillsResolveRequest,
    ) -> TalosResult<talos_sdk::SkillsResolveResponse> {
        self.gateway().resolve_skills(&self.actor, request).await
    }

    async fn list_sessions(
        &mut self,
        request: SessionListRequest,
    ) -> TalosResult<SessionListResponse> {
        self.gateway().list_sessions(&self.actor, request).await
    }

    async fn get_session(&mut self, request: SessionGetRequest) -> TalosResult<SessionGetResponse> {
        self.gateway().get_session(&self.actor, request).await
    }

    async fn trajectory_capabilities(
        &mut self,
    ) -> TalosResult<talos_sdk::trajectory::TrajectoryCapabilitiesResponse> {
        self.gateway().trajectory_capabilities(&self.actor).await
    }
    async fn read_turn_trajectory(
        &mut self,
        request: talos_sdk::trajectory::TrajectoryReadTurnRequest,
    ) -> TalosResult<talos_sdk::trajectory::TrajectoryReadTurnResponse> {
        self.gateway()
            .read_turn_trajectory(&self.actor, request)
            .await
    }
    async fn read_request_trajectory(
        &mut self,
        request: talos_sdk::trajectory::TrajectoryReadModelRequestRequest,
    ) -> TalosResult<talos_sdk::trajectory::TrajectoryReadModelRequestResponse> {
        self.gateway()
            .read_request_trajectory(&self.actor, request)
            .await
    }
    async fn read_tool_trajectory(
        &mut self,
        request: talos_sdk::trajectory::TrajectoryReadToolRequest,
    ) -> TalosResult<talos_sdk::trajectory::TrajectoryReadToolResponse> {
        self.gateway()
            .read_tool_trajectory(&self.actor, request)
            .await
    }
    async fn read_trajectory_payload(
        &mut self,
        request: talos_sdk::trajectory::TrajectoryReadPayloadRequest,
    ) -> TalosResult<talos_sdk::trajectory::TrajectoryReadPayloadResponse> {
        self.gateway()
            .read_trajectory_payload(&self.actor, request)
            .await
    }
    async fn get_session_history(
        &mut self,
        request: SessionHistoryRequest,
    ) -> TalosResult<SessionHistoryResponse> {
        self.gateway()
            .get_session_history(&self.actor, request)
            .await
    }

    async fn get_active_turn(
        &mut self,
        request: SessionActiveTurnRequest,
    ) -> TalosResult<SessionActiveTurnResponse> {
        self.gateway().get_active_turn(&self.actor, request).await
    }

    async fn patch_session(
        &mut self,
        request: SessionPatchRequest,
    ) -> TalosResult<SessionUpdateResponse> {
        self.gateway().patch_session(&self.actor, request).await
    }

    async fn close_session(
        &mut self,
        request: SessionCloseRequest,
    ) -> TalosResult<SessionCloseResponse> {
        self.gateway().close_session(&self.actor, request).await
    }

    async fn delete_session(
        &mut self,
        request: SessionDeleteRequest,
    ) -> TalosResult<SessionDeleteResponse> {
        self.gateway().delete_session(&self.actor, request).await
    }

    async fn compact_session(
        &mut self,
        request: SessionCompactRequest,
    ) -> TalosResult<SessionCompactResponse> {
        self.gateway().compact_session(&self.actor, request).await
    }

    async fn clear_session(
        &mut self,
        request: SessionClearRequest,
    ) -> TalosResult<SessionClearResponse> {
        self.gateway().clear_session(&self.actor, request).await
    }

    async fn recover_timeline(
        &mut self,
        request: RecoverTimelineRequest,
    ) -> TalosResult<RecoverTimelineResponse> {
        self.gateway().recover_timeline(&self.actor, request).await
    }

    async fn subscribe_timeline(
        &mut self,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<SubscribeTimelineResponse> {
        self.gateway()
            .subscribe_timeline(&self.actor, request)
            .await
    }

    async fn open_timeline_stream(
        &mut self,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineStreamSubscription> {
        self.open_embedded_timeline_stream(request).await
    }

    async fn unsubscribe_timeline(
        &mut self,
        request: EventUnsubscribeRequest,
    ) -> TalosResult<EventUnsubscribeResponse> {
        self.unsubscribe_embedded_timeline(request).await
    }

    async fn start_turn(&mut self, request: StartTurnRequest) -> TalosResult<StartTurnResponse> {
        self.start_embedded_turn(request).await
    }

    async fn retry_turn(&mut self, request: RetryTurnRequest) -> TalosResult<StartTurnResponse> {
        self.gateway().retry_turn(&self.actor, request).await
    }

    async fn interrupt_turn(
        &mut self,
        request: InterruptTurnRequest,
    ) -> TalosResult<InterruptTurnResponse> {
        self.gateway().interrupt_turn(&self.actor, request).await
    }

    async fn add_queued_message(
        &mut self,
        request: MessageQueueAddRequest,
    ) -> TalosResult<MessageQueue> {
        self.gateway()
            .add_queued_message(&self.actor, request)
            .await
    }

    async fn list_message_queue(
        &mut self,
        request: MessageQueueListRequest,
    ) -> TalosResult<MessageQueue> {
        self.gateway()
            .list_message_queue(&self.actor, request)
            .await
    }

    async fn update_queued_message(
        &mut self,
        request: MessageQueueUpdateRequest,
    ) -> TalosResult<MessageQueue> {
        self.gateway()
            .update_queued_message(&self.actor, request)
            .await
    }

    async fn delete_queued_message(
        &mut self,
        request: MessageQueueDeleteRequest,
    ) -> TalosResult<MessageQueue> {
        self.gateway()
            .delete_queued_message(&self.actor, request)
            .await
    }

    async fn reorder_message_queue(
        &mut self,
        request: MessageQueueReorderRequest,
    ) -> TalosResult<MessageQueue> {
        self.gateway()
            .reorder_message_queue(&self.actor, request)
            .await
    }

    async fn start_queued_message(
        &mut self,
        request: MessageQueueStartRequest,
    ) -> TalosResult<StartTurnResponse> {
        self.gateway()
            .start_queued_message(&self.actor, request)
            .await
    }

    async fn submit_turn_input(
        &mut self,
        request: SubmitTurnInputRequest,
    ) -> TalosResult<SubmitTurnInputResponse> {
        self.gateway().submit_turn_input(&self.actor, request).await
    }

    async fn list_projects(
        &mut self,
        request: ProjectListRequest,
    ) -> TalosResult<ProjectListResponse> {
        self.gateway().list_projects(&self.actor, request).await
    }

    async fn get_project(
        &mut self,
        request: ProjectReadRequest,
    ) -> TalosResult<ProjectReadResponse> {
        self.gateway().get_project(&self.actor, request).await
    }

    async fn create_project(
        &mut self,
        request: ProjectCreateRequest,
    ) -> TalosResult<ProjectCreateResponse> {
        self.gateway().create_project(&self.actor, request).await
    }

    async fn patch_project(
        &mut self,
        request: ProjectPatchRequest,
    ) -> TalosResult<ProjectPatchResponse> {
        self.gateway().patch_project(&self.actor, request).await
    }

    async fn delete_project(
        &mut self,
        request: ProjectDeleteRequest,
    ) -> TalosResult<ProjectDeleteResponse> {
        self.gateway().delete_project(&self.actor, request).await
    }

    async fn read_fs_directory(
        &mut self,
        request: FsDirectoryReadRequest,
    ) -> TalosResult<FsDirectoryReadResponse> {
        self.gateway().read_fs_directory(&self.actor, request).await
    }

    async fn create_fs_directory(
        &mut self,
        request: FsDirectoryCreateRequest,
    ) -> TalosResult<FsDirectoryCreateResponse> {
        self.gateway()
            .create_fs_directory(&self.actor, request)
            .await
    }

    async fn list_plugin_sources(
        &mut self,
        request: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse> {
        self.gateway()
            .list_plugin_sources(&self.actor, request)
            .await
    }

    async fn add_plugin_source(
        &mut self,
        request: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse> {
        self.gateway().add_plugin_source(&self.actor, request).await
    }

    async fn refresh_plugin_source(
        &mut self,
        request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        self.gateway()
            .refresh_plugin_source(&self.actor, request)
            .await
    }

    async fn remove_plugin_source(
        &mut self,
        request: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse> {
        self.gateway()
            .remove_plugin_source(&self.actor, request)
            .await
    }

    async fn search_plugins(
        &mut self,
        request: PluginSearchRequest,
    ) -> TalosResult<PluginSearchResponse> {
        self.gateway().search_plugins(&self.actor, request).await
    }

    async fn list_plugins(
        &mut self,
        request: PluginListRequest,
    ) -> TalosResult<PluginListResponse> {
        self.gateway().list_plugins(&self.actor, request).await
    }

    async fn read_plugin(&mut self, request: PluginReadRequest) -> TalosResult<PluginReadResponse> {
        self.gateway().read_plugin(&self.actor, request).await
    }

    async fn install_plugin(
        &mut self,
        request: PluginInstallRequest,
    ) -> TalosResult<PluginInstallResponse> {
        self.gateway().install_plugin(&self.actor, request).await
    }

    async fn update_plugin(
        &mut self,
        request: PluginUpdateRequest,
    ) -> TalosResult<PluginUpdateResponse> {
        self.gateway().update_plugin(&self.actor, request).await
    }

    async fn uninstall_plugin(
        &mut self,
        request: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse> {
        self.gateway().uninstall_plugin(&self.actor, request).await
    }

    async fn enable_plugin(
        &mut self,
        request: PluginEnableRequest,
    ) -> TalosResult<PluginEnableResponse> {
        self.gateway().enable_plugin(&self.actor, request).await
    }

    async fn disable_plugin(
        &mut self,
        request: PluginDisableRequest,
    ) -> TalosResult<PluginDisableResponse> {
        self.gateway().disable_plugin(&self.actor, request).await
    }

    async fn upload_file(
        &mut self,
        request: FileUploadRequest,
        data: Bytes,
    ) -> TalosResult<FileUploadResponse> {
        self.upload_embedded_file(request, data).await
    }

    async fn list_files(&mut self, request: FileListRequest) -> TalosResult<FileListResponse> {
        self.gateway().list_files(&self.actor, request).await
    }

    async fn read_file(&mut self, request: FileReadRequest) -> TalosResult<FileReadResponse> {
        self.gateway().read_file(&self.actor, request).await
    }

    async fn read_file_content(
        &mut self,
        request: FileReadRequest,
    ) -> TalosResult<talos_sdk::FileDownloadResponse> {
        self.read_embedded_file_content(request).await
    }

    async fn update_file(&mut self, request: FileUpdateRequest) -> TalosResult<FileUpdateResponse> {
        self.gateway().update_file(&self.actor, request).await
    }

    async fn delete_file(&mut self, request: FileDeleteRequest) -> TalosResult<FileDeleteResponse> {
        self.gateway().delete_file(&self.actor, request).await
    }

    async fn list_memory_files(
        &mut self,
        request: MemoryFileListRequest,
    ) -> TalosResult<MemoryFileListResponse> {
        self.gateway().list_memory_files(&self.actor, request).await
    }

    async fn read_memory_file(
        &mut self,
        request: MemoryFileReadRequest,
    ) -> TalosResult<MemoryFileReadResponse> {
        self.gateway().read_memory_file(&self.actor, request).await
    }

    async fn write_memory_file(
        &mut self,
        request: MemoryFileWriteRequest,
    ) -> TalosResult<MemoryFileWriteResponse> {
        self.gateway().write_memory_file(&self.actor, request).await
    }

    async fn search_memory(
        &mut self,
        request: MemorySearchRequest,
    ) -> TalosResult<MemorySearchResponse> {
        self.gateway().search_memory(&self.actor, request).await
    }

    async fn get_memory(&mut self, request: MemoryGetRequest) -> TalosResult<MemoryGetResponse> {
        self.gateway().get_memory(&self.actor, request).await
    }

    async fn workspace_info(
        &mut self,
        request: WorkspaceInfoRequest,
    ) -> TalosResult<WorkspaceInfoResponse> {
        self.gateway().workspace_info(&self.actor, request).await
    }

    async fn list_workspace_docs(
        &mut self,
        request: WorkspaceDocListRequest,
    ) -> TalosResult<WorkspaceDocListResponse> {
        self.gateway()
            .list_workspace_docs(&self.actor, request)
            .await
    }

    async fn read_workspace_doc(
        &mut self,
        request: WorkspaceDocReadRequest,
    ) -> TalosResult<WorkspaceDocReadResponse> {
        self.gateway()
            .read_workspace_doc(&self.actor, request)
            .await
    }

    async fn write_workspace_doc(
        &mut self,
        request: WorkspaceDocWriteRequest,
    ) -> TalosResult<WorkspaceDocWriteResponse> {
        self.gateway()
            .write_workspace_doc(&self.actor, request)
            .await
    }

    async fn shutdown(&mut self) -> TalosResult<()> {
        self.shutdown_embedded_runtime().await
    }
}
