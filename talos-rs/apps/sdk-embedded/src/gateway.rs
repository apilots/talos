//! Existing product API delegation for this client transport.
use super::*;

mod timeline;
mod transport;

// Generate only identical carrier selection; overrides retain product-specific behavior.
macro_rules! delegate_gateway {
    (
        delegates { $( $name:ident($request:ty) -> $response:ty; )* }
        overrides { $( $override:item )* }
    ) => {
        #[async_trait]
        impl GatewayClient for EmbeddedServerClient {
            $(
                async fn $name(&mut self, request: $request) -> TalosResult<$response> {
                    if let Some(remote) = &self.remote {
                        return remote.lock().await.$name(request).await;
                    }
                    self.gateway().$name(&self.actor, request).await
                }
            )*
            $( $override )*
        }
    };
}

delegate_gateway! {
    delegates {
        session_automation(talos_types::SessionAutomationReadRequest) -> talos_types::SessionAutomationSnapshot;
        execute_automation(talos_types::AutomationRequest) -> talos_types::AutomationControlResult;
        list_automation(talos_types::AutomationListRequest) -> talos_types::AutomationListResponse;
        automation_command_catalog(talos_types::AutomationCommandCatalogRequest) -> talos_types::AutomationCommandCatalogResponse;
        resolve_automation_command(talos_types::AutomationCommandResolveRequest) -> talos_types::AutomationCommandResolveResponse;
        start_managed_worker(talos_types::ManagedWorkerActionRequest) -> talos_types::ManagedWorkerResponse;
        stop_managed_worker(talos_types::ManagedWorkerActionRequest) -> talos_types::ManagedWorkerResponse;
        reset_managed_worker(talos_types::ManagedWorkerActionRequest) -> talos_types::ManagedWorkerResponse;
        delete_managed_worker(talos_types::DeleteManagedWorkerRequest) -> talos_types::ManagedWorkerResponse;
        read_auth_session(AuthSessionReadRequest) -> AuthSessionReadResponse;
        cancel_cloud_login(CloudLoginCancelRequest) -> CloudLoginCancelResponse;
        get_session(SessionGetRequest) -> SessionGetResponse;
        read_turn_trajectory(talos_sdk::trajectory::TrajectoryReadTurnRequest) -> talos_sdk::trajectory::TrajectoryReadTurnResponse;
        read_request_trajectory(talos_sdk::trajectory::TrajectoryReadModelRequestRequest) -> talos_sdk::trajectory::TrajectoryReadModelRequestResponse;
        read_tool_trajectory(talos_sdk::trajectory::TrajectoryReadToolRequest) -> talos_sdk::trajectory::TrajectoryReadToolResponse;
        read_trajectory_payload(talos_sdk::trajectory::TrajectoryReadPayloadRequest) -> talos_sdk::trajectory::TrajectoryReadPayloadResponse;
        get_session_history(SessionHistoryRequest) -> SessionHistoryResponse;
        get_active_turn(SessionActiveTurnRequest) -> SessionActiveTurnResponse;
        patch_session(SessionPatchRequest) -> SessionUpdateResponse;
        close_session(SessionCloseRequest) -> SessionCloseResponse;
        delete_session(SessionDeleteRequest) -> SessionDeleteResponse;
        compact_session(SessionCompactRequest) -> SessionCompactResponse;
        clear_session(SessionClearRequest) -> SessionClearResponse;
        rebuild_session_view(talos_types::SessionRebuildViewRequest) -> talos_types::SessionRebuildViewResponse;
        recover_timeline(RecoverTimelineRequest) -> RecoverTimelineResponse;
        subscribe_timeline(SubscribeTimelineRequest) -> SubscribeTimelineResponse;
        retry_turn(RetryTurnRequest) -> StartTurnResponse;
        interrupt_turn(InterruptTurnRequest) -> InterruptTurnResponse;
        add_queued_message(MessageQueueAddRequest) -> MessageQueue;
        list_message_queue(MessageQueueListRequest) -> MessageQueue;
        update_queued_message(MessageQueueUpdateRequest) -> MessageQueue;
        delete_queued_message(MessageQueueDeleteRequest) -> MessageQueue;
        reorder_message_queue(MessageQueueReorderRequest) -> MessageQueue;
        start_queued_message(MessageQueueStartRequest) -> StartTurnResponse;
        submit_turn_input(SubmitTurnInputRequest) -> SubmitTurnInputResponse;
        get_project(ProjectReadRequest) -> ProjectReadResponse;
        patch_project(ProjectPatchRequest) -> ProjectPatchResponse;
        delete_project(ProjectDeleteRequest) -> ProjectDeleteResponse;
        list_files(FileListRequest) -> FileListResponse;
        read_file(FileReadRequest) -> FileReadResponse;
        update_file(FileUpdateRequest) -> FileUpdateResponse;
        delete_file(FileDeleteRequest) -> FileDeleteResponse;
    }
    overrides {
        async fn list_builtin_jobs(
            &mut self,
            mut request: talos_types::BuiltinJobListRequest,
        ) -> TalosResult<talos_types::BuiltinJobListResponse> {
            self.select_computer(&request.computer_id)?;
            if self.remote.is_none() {
                request.computer_id = "embedded".into();
            }
            if let Some(remote) = &self.remote {
                return remote.lock().await.list_builtin_jobs(request).await;
            }

            self.gateway().list_builtin_jobs(&self.actor, request).await
        }

        async fn control_builtin_job(
            &mut self,
            mut request: talos_types::BuiltinJobControlRequest,
        ) -> TalosResult<talos_types::BuiltinJobInfo> {
            self.select_computer(&request.computer_id)?;
            if self.remote.is_none() {
                request.computer_id = "embedded".into();
            }
            if let Some(remote) = &self.remote {
                return remote.lock().await.control_builtin_job(request).await;
            }

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
            if let Some(mut remote) = self.control_plane_client()? {
                return remote.list_computers(request).await;
            }

            self.gateway().list_computers(&self.actor, request).await
        }

        async fn read_computer(
            &mut self,
            request: talos_types::GetNodeRequest,
        ) -> TalosResult<talos_types::NodeInfo> {
            if let Some(mut remote) = self.control_plane_client()? {
                return remote.read_computer(request).await;
            }

            self.gateway().read_computer(&self.actor, request).await
        }

        async fn create_computer(
            &mut self,
            request: talos_types::CreateNodeRequest,
        ) -> TalosResult<talos_types::CreatedNode> {
            if let Some(mut remote) = self.control_plane_client()? {
                return remote.create_computer(request).await;
            }

            self.gateway().create_computer(&self.actor, request).await
        }

        async fn rename_computer(
            &mut self,
            request: talos_types::RenameComputerRequest,
        ) -> TalosResult<talos_types::NodeInfo> {
            if let Some(mut remote) = self.control_plane_client()? {
                return remote.rename_computer(request).await;
            }

            self.gateway().rename_computer(&self.actor, request).await
        }

        async fn delete_computer(
            &mut self,
            request: talos_types::DeleteComputerRequest,
        ) -> TalosResult<talos_types::DeleteComputerResponse> {
            if let Some(mut remote) = self.control_plane_client()? {
                return remote.delete_computer(request).await;
            }

            self.gateway().delete_computer(&self.actor, request).await
        }

        async fn set_computer_status(
            &mut self,
            request: talos_types::SetComputerStatusRequest,
        ) -> TalosResult<talos_types::NodeInfo> {
            if let Some(mut remote) = self.control_plane_client()? {
                return remote.set_computer_status(request).await;
            }

            self.gateway()
                .set_computer_status(&self.actor, request)
                .await
        }

        async fn rotate_computer_credential(
            &mut self,
            request: talos_types::RotateCredentialRequest,
        ) -> TalosResult<talos_types::NodeCredential> {
            if let Some(mut remote) = self.control_plane_client()? {
                return remote.rotate_computer_credential(request).await;
            }

            self.gateway()
                .rotate_computer_credential(&self.actor, request)
                .await
        }

        async fn import_mcp_server(
            &mut self,
            mut request: talos_sdk::McpServerImportRequest,
        ) -> TalosResult<talos_sdk::PluginInstallResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.import_mcp_server(request).await;
            }

            self.gateway().import_mcp_server(&self.actor, request).await
        }

        async fn start_mcp_authentication(
            &mut self,
            mut request: talos_sdk::McpAuthenticationStartRequest,
        ) -> TalosResult<talos_sdk::McpAuthenticationStartResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.start_mcp_authentication(request).await;
            }

            self.gateway()
                .start_mcp_authentication(&self.actor, request)
                .await
        }

        async fn read_mcp_authentication(
            &mut self,
            mut request: talos_sdk::McpAuthenticationReadRequest,
        ) -> TalosResult<talos_sdk::McpAuthenticationResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.read_mcp_authentication(request).await;
            }

            self.gateway()
                .read_mcp_authentication(&self.actor, request)
                .await
        }

        async fn update_mcp_credentials(
            &mut self,
            mut request: talos_sdk::McpCredentialsUpdateRequest,
        ) -> TalosResult<talos_sdk::McpAuthenticationResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.update_mcp_credentials(request).await;
            }

            self.gateway()
                .update_mcp_credentials(&self.actor, request)
                .await
        }

        async fn open_desktop(
            &mut self,
            mut request: talos_sdk::DesktopOpenRequest,
        ) -> TalosResult<talos_sdk::DesktopViewGrant> {
            self.select_computer(&request.node_id)?;
            if self.remote.is_none() {
                request.node_id = "embedded".into();
            }
            if let Some(remote) = &self.remote {
                return remote.lock().await.open_desktop(request).await;
            }

            self.gateway().open_desktop(&self.actor, request).await
        }

        async fn browser(
            &mut self,
            mut request: talos_sdk::BrowserRequest,
        ) -> TalosResult<talos_sdk::BrowserSnapshot> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.browser(request).await;
            }

            self.gateway().browser(&self.actor, request).await
        }

        async fn list_mcp_servers(
            &mut self,
            mut request: talos_sdk::McpServerListRequest,
        ) -> TalosResult<talos_sdk::McpServerListResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.list_mcp_servers(request).await;
            }

            self.gateway().list_mcp_servers(&self.actor, request).await
        }

        async fn read_managed_worker(&mut self) -> TalosResult<talos_types::ManagedWorkerResponse> {
            if let Some(remote) = &self.remote {
                return remote.lock().await.read_managed_worker().await;
            }

            self.gateway().read_managed_worker(&self.actor).await
        }

        async fn create_managed_worker(&mut self) -> TalosResult<talos_types::ManagedWorkerResponse> {
            if let Some(remote) = &self.remote {
                return remote.lock().await.create_managed_worker().await;
            }

            self.gateway().create_managed_worker(&self.actor).await
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
            if let Some(remote) = &self.remote {
                return remote.lock().await.start_cloud_login(request).await;
            }

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

        async fn list_models(
            &mut self,
            mut request: ModelListRequest,
        ) -> TalosResult<ModelListResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.list_models(request).await;
            }

            self.gateway().list_models(&self.actor, request).await
        }

        async fn get_config(&mut self, mut request: ConfigGetRequest) -> TalosResult<ConfigResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.get_config(request).await;
            }

            self.gateway().get_config(&self.actor, request).await
        }

        async fn update_config(
            &mut self,
            mut request: ConfigUpdateRequest,
        ) -> TalosResult<ConfigResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.update_config(request).await;
            }

            self.gateway().update_config(&self.actor, request).await
        }

        async fn delete_config(
            &mut self,
            mut request: ConfigDeleteRequest,
        ) -> TalosResult<ConfigDeleteResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.delete_config(request).await;
            }

            self.gateway().delete_config(&self.actor, request).await
        }

        async fn reload_config(
            &mut self,
            mut request: ConfigReloadRequest,
        ) -> TalosResult<ConfigReloadResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.reload_config(request).await;
            }

            self.gateway().reload_config(&self.actor, request).await
        }

        async fn list_skills(
            &mut self,
            mut request: talos_sdk::SkillsListRequest,
        ) -> TalosResult<talos_sdk::SkillsListResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.list_skills(request).await;
            }

            self.gateway().list_skills(&self.actor, request).await
        }

        async fn refresh_skills(
            &mut self,
            mut request: talos_sdk::SkillsRefreshRequest,
        ) -> TalosResult<talos_sdk::SkillsListResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.refresh_skills(request).await;
            }

            self.gateway().refresh_skills(&self.actor, request).await
        }

        async fn suggest_skills(
            &mut self,
            mut request: talos_sdk::SkillsSuggestRequest,
        ) -> TalosResult<talos_sdk::SkillsSuggestResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.suggest_skills(request).await;
            }

            self.gateway().suggest_skills(&self.actor, request).await
        }

        async fn read_skill(
            &mut self,
            mut request: talos_sdk::SkillsReadRequest,
        ) -> TalosResult<SkillsReadResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.read_skill(request).await;
            }

            self.gateway().read_skill(&self.actor, request).await
        }

        async fn resolve_skills(
            &mut self,
            mut request: talos_sdk::SkillsResolveRequest,
        ) -> TalosResult<talos_sdk::SkillsResolveResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.resolve_skills(request).await;
            }

            self.gateway().resolve_skills(&self.actor, request).await
        }

        async fn list_sessions(
            &mut self,
            mut request: SessionListRequest,
        ) -> TalosResult<SessionListResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.list_sessions(request).await;
            }

            self.gateway().list_sessions(&self.actor, request).await
        }

        async fn trajectory_capabilities(
            &mut self,
        ) -> TalosResult<talos_sdk::trajectory::TrajectoryCapabilitiesResponse> {
            if let Some(remote) = &self.remote {
                return remote.lock().await.trajectory_capabilities().await;
            }

            self.gateway().trajectory_capabilities(&self.actor).await
        }

        async fn open_timeline_stream(
            &mut self,
            request: SubscribeTimelineRequest,
        ) -> TalosResult<TimelineStreamSubscription> {
            if let Some(remote) = &self.remote {
                return remote.lock().await.open_timeline_stream(request).await;
            }

            self.open_embedded_timeline_stream(request).await
        }

        async fn unsubscribe_timeline(
            &mut self,
            request: EventUnsubscribeRequest,
        ) -> TalosResult<EventUnsubscribeResponse> {
            if let Some(remote) = &self.remote {
                return remote.lock().await.unsubscribe_timeline(request).await;
            }

            self.unsubscribe_embedded_timeline(request).await
        }

        async fn start_turn(
            &mut self,
            mut request: StartTurnRequest,
        ) -> TalosResult<StartTurnResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.start_turn(request).await;
            }

            self.start_embedded_turn(request).await
        }

        async fn list_projects(
            &mut self,
            mut request: ProjectListRequest,
        ) -> TalosResult<ProjectListResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.list_projects(request).await;
            }

            self.gateway().list_projects(&self.actor, request).await
        }

        async fn create_project(
            &mut self,
            mut request: ProjectCreateRequest,
        ) -> TalosResult<ProjectCreateResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.create_project(request).await;
            }

            self.gateway().create_project(&self.actor, request).await
        }

        async fn read_fs_directory(
            &mut self,
            mut request: FsDirectoryReadRequest,
        ) -> TalosResult<FsDirectoryReadResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.read_fs_directory(request).await;
            }

            self.gateway().read_fs_directory(&self.actor, request).await
        }

        async fn create_fs_directory(
            &mut self,
            mut request: FsDirectoryCreateRequest,
        ) -> TalosResult<FsDirectoryCreateResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.create_fs_directory(request).await;
            }

            self.gateway()
                .create_fs_directory(&self.actor, request)
                .await
        }

        async fn list_plugin_sources(
            &mut self,
            mut request: PluginSourceListRequest,
        ) -> TalosResult<PluginSourceListResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.list_plugin_sources(request).await;
            }

            self.gateway()
                .list_plugin_sources(&self.actor, request)
                .await
        }

        async fn add_plugin_source(
            &mut self,
            mut request: PluginSourceAddRequest,
        ) -> TalosResult<PluginSourceAddResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.add_plugin_source(request).await;
            }

            self.gateway().add_plugin_source(&self.actor, request).await
        }

        async fn refresh_plugin_source(
            &mut self,
            mut request: PluginSourceRefreshRequest,
        ) -> TalosResult<PluginSourceRefreshResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.refresh_plugin_source(request).await;
            }

            self.gateway()
                .refresh_plugin_source(&self.actor, request)
                .await
        }

        async fn remove_plugin_source(
            &mut self,
            mut request: PluginSourceRemoveRequest,
        ) -> TalosResult<PluginSourceRemoveResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.remove_plugin_source(request).await;
            }

            self.gateway()
                .remove_plugin_source(&self.actor, request)
                .await
        }

        async fn search_plugins(
            &mut self,
            mut request: PluginSearchRequest,
        ) -> TalosResult<PluginSearchResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.search_plugins(request).await;
            }

            self.gateway().search_plugins(&self.actor, request).await
        }

        async fn list_plugins(
            &mut self,
            mut request: PluginListRequest,
        ) -> TalosResult<PluginListResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.list_plugins(request).await;
            }

            self.gateway().list_plugins(&self.actor, request).await
        }

        async fn read_plugin(
            &mut self,
            mut request: PluginReadRequest,
        ) -> TalosResult<PluginReadResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.read_plugin(request).await;
            }

            self.gateway().read_plugin(&self.actor, request).await
        }

        async fn install_plugin(
            &mut self,
            mut request: PluginInstallRequest,
        ) -> TalosResult<PluginInstallResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.install_plugin(request).await;
            }

            self.gateway().install_plugin(&self.actor, request).await
        }

        async fn update_plugin(
            &mut self,
            mut request: PluginUpdateRequest,
        ) -> TalosResult<PluginUpdateResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.update_plugin(request).await;
            }

            self.gateway().update_plugin(&self.actor, request).await
        }

        async fn uninstall_plugin(
            &mut self,
            mut request: PluginUninstallRequest,
        ) -> TalosResult<PluginUninstallResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.uninstall_plugin(request).await;
            }

            self.gateway().uninstall_plugin(&self.actor, request).await
        }

        async fn enable_plugin(
            &mut self,
            mut request: PluginEnableRequest,
        ) -> TalosResult<PluginEnableResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.enable_plugin(request).await;
            }

            self.gateway().enable_plugin(&self.actor, request).await
        }

        async fn disable_plugin(
            &mut self,
            mut request: PluginDisableRequest,
        ) -> TalosResult<PluginDisableResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.disable_plugin(request).await;
            }

            self.gateway().disable_plugin(&self.actor, request).await
        }

        async fn upload_file(
            &mut self,
            request: FileUploadRequest,
            data: Bytes,
        ) -> TalosResult<FileUploadResponse> {
            if let Some(remote) = &self.remote {
                return remote.lock().await.upload_file(request, data).await;
            }

            self.upload_embedded_file(request, data).await
        }

        async fn read_file_content(
            &mut self,
            request: FileReadRequest,
        ) -> TalosResult<talos_sdk::FileDownloadResponse> {
            if let Some(remote) = &self.remote {
                return remote.lock().await.read_file_content(request).await;
            }

            self.read_embedded_file_content(request).await
        }

        async fn list_memory_files(
            &mut self,
            mut request: MemoryFileListRequest,
        ) -> TalosResult<MemoryFileListResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.list_memory_files(request).await;
            }

            self.gateway().list_memory_files(&self.actor, request).await
        }

        async fn read_memory_file(
            &mut self,
            mut request: MemoryFileReadRequest,
        ) -> TalosResult<MemoryFileReadResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.read_memory_file(request).await;
            }

            self.gateway().read_memory_file(&self.actor, request).await
        }

        async fn write_memory_file(
            &mut self,
            mut request: MemoryFileWriteRequest,
        ) -> TalosResult<MemoryFileWriteResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.write_memory_file(request).await;
            }

            self.gateway().write_memory_file(&self.actor, request).await
        }

        async fn search_memory(
            &mut self,
            mut request: MemorySearchRequest,
        ) -> TalosResult<MemorySearchResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.search_memory(request).await;
            }

            self.gateway().search_memory(&self.actor, request).await
        }

        async fn get_memory(
            &mut self,
            mut request: MemoryGetRequest,
        ) -> TalosResult<MemoryGetResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.get_memory(request).await;
            }

            self.gateway().get_memory(&self.actor, request).await
        }

        async fn workspace_info(
            &mut self,
            mut request: WorkspaceInfoRequest,
        ) -> TalosResult<WorkspaceInfoResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.workspace_info(request).await;
            }

            self.gateway().workspace_info(&self.actor, request).await
        }

        async fn list_workspace_docs(
            &mut self,
            mut request: WorkspaceDocListRequest,
        ) -> TalosResult<WorkspaceDocListResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.list_workspace_docs(request).await;
            }

            self.gateway()
                .list_workspace_docs(&self.actor, request)
                .await
        }

        async fn read_workspace_doc(
            &mut self,
            mut request: WorkspaceDocReadRequest,
        ) -> TalosResult<WorkspaceDocReadResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.read_workspace_doc(request).await;
            }

            self.gateway()
                .read_workspace_doc(&self.actor, request)
                .await
        }

        async fn write_workspace_doc(
            &mut self,
            mut request: WorkspaceDocWriteRequest,
        ) -> TalosResult<WorkspaceDocWriteResponse> {
            self.select_optional_computer(&mut request.node_id)?;
            if let Some(remote) = &self.remote {
                return remote.lock().await.write_workspace_doc(request).await;
            }

            self.gateway()
                .write_workspace_doc(&self.actor, request)
                .await
        }

        async fn shutdown(&mut self) -> TalosResult<()> {
            self.shutdown_embedded_runtime().await
        }
    }
}
