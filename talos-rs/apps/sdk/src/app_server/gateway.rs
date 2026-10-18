//! Existing product API delegation for this client transport.
use super::*;

#[async_trait]
impl GatewayClient for AppServerClient {
    async fn sync_metrics(&mut self) -> TalosResult<talos_types::SyncMetricsResponse> {
        self.request_typed("sync/metrics", Option::<()>::None).await
    }

    async fn list_computers(
        &mut self,
        request: talos_types::ComputerListRequest,
    ) -> TalosResult<talos_types::ComputerListResponse> {
        self.request_typed("computer/list", request).await
    }

    async fn read_computer(
        &mut self,
        request: talos_types::GetNodeRequest,
    ) -> TalosResult<talos_types::NodeInfo> {
        self.request_typed("computer/read", request).await
    }

    async fn create_computer(
        &mut self,
        request: talos_types::CreateNodeRequest,
    ) -> TalosResult<talos_types::CreatedNode> {
        self.request_typed("computer/create", request).await
    }

    async fn rename_computer(
        &mut self,
        request: talos_types::RenameComputerRequest,
    ) -> TalosResult<talos_types::NodeInfo> {
        self.request_typed("computer/rename", request).await
    }

    async fn delete_computer(
        &mut self,
        request: talos_types::DeleteComputerRequest,
    ) -> TalosResult<talos_types::DeleteComputerResponse> {
        self.request_typed("computer/delete", request).await
    }

    async fn set_computer_status(
        &mut self,
        request: talos_types::SetComputerStatusRequest,
    ) -> TalosResult<talos_types::NodeInfo> {
        self.request_typed("computer/setStatus", request).await
    }

    async fn rotate_computer_credential(
        &mut self,
        request: talos_types::RotateCredentialRequest,
    ) -> TalosResult<talos_types::NodeCredential> {
        self.request_typed("computer/rotateCredential", request)
            .await
    }

    async fn read_auth_session(
        &mut self,
        request: AuthSessionReadRequest,
    ) -> TalosResult<AuthSessionReadResponse> {
        self.request_typed("auth/session/read", request).await
    }

    async fn clear_auth_session(
        &mut self,
        request: AuthSessionClearRequest,
    ) -> TalosResult<AuthSessionClearResponse> {
        let response = self.request_typed("auth/session/clear", request).await?;
        self.actor_credentials = None;
        Ok(response)
    }

    async fn auth_logout(&mut self, request: AuthLogoutRequest) -> TalosResult<AuthLogoutResponse> {
        let response = self.request_typed("auth/logout", request).await?;
        self.actor_credentials = None;
        Ok(response)
    }

    async fn start_cloud_login(
        &mut self,
        request: CloudLoginStartRequest,
    ) -> TalosResult<CloudLoginStartResponse> {
        self.request_typed("auth/login/start", request).await
    }

    async fn cloud_login_status(
        &mut self,
        request: CloudLoginStatusRequest,
    ) -> TalosResult<CloudLoginStatusResponse> {
        let response = self.request_typed("auth/login/status", request).await?;
        self.apply_cloud_login_credentials(&response).await?;
        Ok(response)
    }

    async fn cancel_cloud_login(
        &mut self,
        request: CloudLoginCancelRequest,
    ) -> TalosResult<CloudLoginCancelResponse> {
        self.request_typed("auth/login/cancel", request).await
    }

    async fn current_user(
        &mut self,
        request: UserCurrentRequest,
    ) -> TalosResult<UserCurrentResponse> {
        self.request_typed("user/current", request).await
    }

    async fn read_profile(
        &mut self,
        request: ProfileReadRequest,
    ) -> TalosResult<ProfileReadResponse> {
        self.request_typed("profile/read", request).await
    }

    async fn list_models(&mut self, request: ModelListRequest) -> TalosResult<ModelListResponse> {
        self.request_typed("model/list", request).await
    }

    async fn get_config(&mut self, request: ConfigGetRequest) -> TalosResult<ConfigResponse> {
        self.request_typed("config/get", request).await
    }

    async fn update_config(&mut self, request: ConfigUpdateRequest) -> TalosResult<ConfigResponse> {
        self.request_typed("config/update", request).await
    }

    async fn delete_config(
        &mut self,
        request: ConfigDeleteRequest,
    ) -> TalosResult<ConfigDeleteResponse> {
        self.request_typed("config/delete", request).await
    }

    async fn reload_config(
        &mut self,
        request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse> {
        self.request_typed("config/reload", request).await
    }

    async fn list_skills(&mut self, request: SkillsListRequest) -> TalosResult<SkillsListResponse> {
        self.request_typed("skills/list", request).await
    }

    async fn refresh_skills(
        &mut self,
        request: SkillsRefreshRequest,
    ) -> TalosResult<SkillsListResponse> {
        self.request_typed("skills/refresh", request).await
    }

    async fn suggest_skills(
        &mut self,
        request: SkillsSuggestRequest,
    ) -> TalosResult<SkillsSuggestResponse> {
        self.request_typed("skills/suggest", request).await
    }

    async fn read_skill(&mut self, request: SkillsReadRequest) -> TalosResult<SkillsReadResponse> {
        self.request_typed("skills/read", request).await
    }

    async fn resolve_skills(
        &mut self,
        request: SkillsResolveRequest,
    ) -> TalosResult<SkillsResolveResponse> {
        self.request_typed("skills/resolve", request).await
    }

    async fn list_sessions(
        &mut self,
        request: SessionListRequest,
    ) -> TalosResult<SessionListResponse> {
        self.request_typed("session/list", request).await
    }

    async fn get_session(&mut self, request: SessionGetRequest) -> TalosResult<SessionGetResponse> {
        self.request_typed("session/get", request).await
    }

    async fn get_session_history(
        &mut self,
        request: SessionHistoryRequest,
    ) -> TalosResult<SessionHistoryResponse> {
        self.request_typed("session/history", request).await
    }

    async fn get_active_turn(
        &mut self,
        request: SessionActiveTurnRequest,
    ) -> TalosResult<SessionActiveTurnResponse> {
        self.request_typed("session/active_turn", request).await
    }

    async fn patch_session(
        &mut self,
        request: SessionPatchRequest,
    ) -> TalosResult<SessionUpdateResponse> {
        self.request_typed("session/patch", request).await
    }

    async fn close_session(
        &mut self,
        request: SessionCloseRequest,
    ) -> TalosResult<SessionCloseResponse> {
        self.request_typed("session/close", request).await
    }

    async fn delete_session(
        &mut self,
        request: SessionDeleteRequest,
    ) -> TalosResult<SessionDeleteResponse> {
        self.request_typed("session/delete", request).await
    }

    async fn compact_session(
        &mut self,
        request: SessionCompactRequest,
    ) -> TalosResult<SessionCompactResponse> {
        self.request_typed("session/compact", request).await
    }

    async fn clear_session(
        &mut self,
        request: SessionClearRequest,
    ) -> TalosResult<SessionClearResponse> {
        self.request_typed("session/clear", request).await
    }

    async fn recover_timeline(
        &mut self,
        request: RecoverTimelineRequest,
    ) -> TalosResult<RecoverTimelineResponse> {
        self.request_typed("timeline/recover", request).await
    }

    async fn subscribe_timeline(
        &mut self,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<SubscribeTimelineResponse> {
        self.request_typed("event/subscribe", request).await
    }

    async fn open_timeline_stream(
        &mut self,
        request: SubscribeTimelineRequest,
    ) -> TalosResult<TimelineStreamSubscription> {
        let session_id = request.session_id.clone();
        let response = self.subscribe_timeline(request).await?;
        let subscription_id = response.subscription_id.clone();
        let (tx, rx) = mpsc::channel(256);
        let (message_queue_tx, message_queue_rx) = mpsc::channel(256);
        {
            let mut streams = self.subscription_senders.lock().await;
            let buffered_events = self
                .pending_subscription_events
                .lock()
                .await
                .remove(&subscription_id)
                .unwrap_or_default();
            for event in buffered_events {
                if event.event.identity().session_id == session_id && tx.try_send(event).is_err() {
                    break;
                }
            }
            streams.insert(
                subscription_id,
                SubscriptionStream {
                    session_id,
                    sender: tx,
                    message_queue_sender: message_queue_tx,
                },
            );
        }
        Ok(TimelineStreamSubscription {
            response,
            stream: TimelineEventStream::new(rx),
            message_queue_changes: MessageQueueChangeStream::new(message_queue_rx),
        })
    }

    async fn unsubscribe_timeline(
        &mut self,
        request: EventUnsubscribeRequest,
    ) -> TalosResult<EventUnsubscribeResponse> {
        self.subscription_senders
            .lock()
            .await
            .remove(&request.subscription_id);
        self.pending_subscription_events
            .lock()
            .await
            .remove(&request.subscription_id);
        self.request_typed("event/unsubscribe", request).await
    }

    async fn start_turn(&mut self, request: StartTurnRequest) -> TalosResult<StartTurnResponse> {
        self.request_typed("turn/start", request).await
    }

    async fn retry_turn(&mut self, request: RetryTurnRequest) -> TalosResult<StartTurnResponse> {
        self.request_typed("turn/retry", request).await
    }

    async fn interrupt_turn(
        &mut self,
        request: InterruptTurnRequest,
    ) -> TalosResult<InterruptTurnResponse> {
        self.request_typed("turn/interrupt", request).await
    }

    async fn add_queued_message(
        &mut self,
        request: MessageQueueAddRequest,
    ) -> TalosResult<MessageQueue> {
        self.request_typed("messageQueue/add", request).await
    }

    async fn list_message_queue(
        &mut self,
        request: MessageQueueListRequest,
    ) -> TalosResult<MessageQueue> {
        self.request_typed("messageQueue/list", request).await
    }

    async fn update_queued_message(
        &mut self,
        request: MessageQueueUpdateRequest,
    ) -> TalosResult<MessageQueue> {
        self.request_typed("messageQueue/update", request).await
    }

    async fn delete_queued_message(
        &mut self,
        request: MessageQueueDeleteRequest,
    ) -> TalosResult<MessageQueue> {
        self.request_typed("messageQueue/delete", request).await
    }

    async fn reorder_message_queue(
        &mut self,
        request: MessageQueueReorderRequest,
    ) -> TalosResult<MessageQueue> {
        self.request_typed("messageQueue/reorder", request).await
    }

    async fn start_queued_message(
        &mut self,
        request: MessageQueueStartRequest,
    ) -> TalosResult<StartTurnResponse> {
        self.request_typed("messageQueue/start", request).await
    }

    async fn submit_turn_input(
        &mut self,
        request: SubmitTurnInputRequest,
    ) -> TalosResult<SubmitTurnInputResponse> {
        self.request_typed("turn/input", request).await
    }

    async fn list_projects(
        &mut self,
        request: ProjectListRequest,
    ) -> TalosResult<ProjectListResponse> {
        self.request_typed("project/list", request).await
    }

    async fn get_project(
        &mut self,
        request: ProjectReadRequest,
    ) -> TalosResult<ProjectReadResponse> {
        self.request_typed("project/read", request).await
    }

    async fn create_project(
        &mut self,
        request: ProjectCreateRequest,
    ) -> TalosResult<ProjectCreateResponse> {
        self.request_typed("project/create", request).await
    }

    async fn patch_project(
        &mut self,
        request: ProjectPatchRequest,
    ) -> TalosResult<ProjectPatchResponse> {
        self.request_typed("project/patch", request).await
    }

    async fn delete_project(
        &mut self,
        request: ProjectDeleteRequest,
    ) -> TalosResult<ProjectDeleteResponse> {
        self.request_typed("project/delete", request).await
    }

    async fn read_fs_directory(
        &mut self,
        request: FsDirectoryReadRequest,
    ) -> TalosResult<FsDirectoryReadResponse> {
        self.request_typed("fs/directory/read", request).await
    }

    async fn create_fs_directory(
        &mut self,
        request: FsDirectoryCreateRequest,
    ) -> TalosResult<FsDirectoryCreateResponse> {
        self.request_typed("fs/directory/create", request).await
    }

    async fn list_plugin_sources(
        &mut self,
        request: PluginSourceListRequest,
    ) -> TalosResult<PluginSourceListResponse> {
        self.request_typed("plugin/source/list", request).await
    }

    async fn add_plugin_source(
        &mut self,
        request: PluginSourceAddRequest,
    ) -> TalosResult<PluginSourceAddResponse> {
        self.request_typed("plugin/source/add", request).await
    }

    async fn refresh_plugin_source(
        &mut self,
        request: PluginSourceRefreshRequest,
    ) -> TalosResult<PluginSourceRefreshResponse> {
        self.request_typed("plugin/source/refresh", request).await
    }

    async fn remove_plugin_source(
        &mut self,
        request: PluginSourceRemoveRequest,
    ) -> TalosResult<PluginSourceRemoveResponse> {
        self.request_typed("plugin/source/remove", request).await
    }

    async fn search_plugins(
        &mut self,
        request: PluginSearchRequest,
    ) -> TalosResult<PluginSearchResponse> {
        self.request_typed("plugin/search", request).await
    }

    async fn import_mcp_server(
        &mut self,
        request: talos_types::McpServerImportRequest,
    ) -> TalosResult<talos_types::PluginInstallResponse> {
        self.request_typed("mcp/server/import", request).await
    }

    async fn start_mcp_authentication(
        &mut self,
        request: talos_types::McpAuthenticationStartRequest,
    ) -> TalosResult<talos_types::McpAuthenticationStartResponse> {
        self.request_typed("mcp/server/auth/start", request).await
    }

    async fn read_mcp_authentication(
        &mut self,
        request: talos_types::McpAuthenticationReadRequest,
    ) -> TalosResult<talos_types::McpAuthenticationResponse> {
        self.request_typed("mcp/server/auth/read", request).await
    }

    async fn update_mcp_credentials(
        &mut self,
        request: talos_types::McpCredentialsUpdateRequest,
    ) -> TalosResult<talos_types::McpAuthenticationResponse> {
        self.request_typed("mcp/server/credentials/update", request)
            .await
    }

    async fn list_mcp_servers(
        &mut self,
        request: talos_types::McpServerListRequest,
    ) -> TalosResult<talos_types::McpServerListResponse> {
        self.request_typed("mcp/server/list", request).await
    }

    async fn list_plugins(
        &mut self,
        request: PluginListRequest,
    ) -> TalosResult<PluginListResponse> {
        self.request_typed("plugin/list", request).await
    }

    async fn read_plugin(&mut self, request: PluginReadRequest) -> TalosResult<PluginReadResponse> {
        self.request_typed("plugin/read", request).await
    }

    async fn install_plugin(
        &mut self,
        request: PluginInstallRequest,
    ) -> TalosResult<PluginInstallResponse> {
        self.request_typed("plugin/install", request).await
    }

    async fn update_plugin(
        &mut self,
        request: PluginUpdateRequest,
    ) -> TalosResult<PluginUpdateResponse> {
        self.request_typed("plugin/update", request).await
    }

    async fn uninstall_plugin(
        &mut self,
        request: PluginUninstallRequest,
    ) -> TalosResult<PluginUninstallResponse> {
        self.request_typed("plugin/uninstall", request).await
    }

    async fn enable_plugin(
        &mut self,
        request: PluginEnableRequest,
    ) -> TalosResult<PluginEnableResponse> {
        self.request_typed("plugin/enable", request).await
    }

    async fn disable_plugin(
        &mut self,
        request: PluginDisableRequest,
    ) -> TalosResult<PluginDisableResponse> {
        self.request_typed("plugin/disable", request).await
    }

    async fn upload_file(
        &mut self,
        request: talos_types::FileUploadRequest,
        data: bytes::Bytes,
    ) -> TalosResult<talos_types::FileUploadResponse> {
        self.request_typed("file/upload", files::file_upload_params(request, data))
            .await
    }

    async fn list_files(
        &mut self,
        request: talos_types::FileListRequest,
    ) -> TalosResult<talos_types::FileListResponse> {
        self.request_typed("file/list", request).await
    }

    async fn read_file(
        &mut self,
        request: talos_types::FileReadRequest,
    ) -> TalosResult<talos_types::FileReadResponse> {
        self.request_typed("file/read", request).await
    }

    async fn read_file_content(
        &mut self,
        request: talos_types::FileReadRequest,
    ) -> TalosResult<crate::FileDownloadResponse> {
        use base64::Engine;
        let response: talos_types::FileContentResponse =
            self.request_typed("file/content", request).await?;
        let data = base64::engine::general_purpose::STANDARD
            .decode(response.data_base64)
            .map_err(|error| {
                TalosError::internal_with_source(
                    "File content response contains invalid base64.",
                    error,
                )
            })?;
        Ok(crate::FileDownloadResponse {
            file: response.file,
            data: data.into(),
        })
    }

    async fn update_file(
        &mut self,
        request: talos_types::FileUpdateRequest,
    ) -> TalosResult<talos_types::FileUpdateResponse> {
        self.request_typed("file/update", request).await
    }

    async fn delete_file(
        &mut self,
        request: talos_types::FileDeleteRequest,
    ) -> TalosResult<talos_types::FileDeleteResponse> {
        self.request_typed("file/delete", request).await
    }

    async fn list_memory_files(
        &mut self,
        request: MemoryFileListRequest,
    ) -> TalosResult<MemoryFileListResponse> {
        self.request_typed("memory/file/list", request).await
    }

    async fn read_memory_file(
        &mut self,
        request: MemoryFileReadRequest,
    ) -> TalosResult<MemoryFileReadResponse> {
        self.request_typed("memory/file/read", request).await
    }

    async fn write_memory_file(
        &mut self,
        request: MemoryFileWriteRequest,
    ) -> TalosResult<MemoryFileWriteResponse> {
        self.request_typed("memory/file/write", request).await
    }

    async fn search_memory(
        &mut self,
        request: MemorySearchRequest,
    ) -> TalosResult<MemorySearchResponse> {
        self.request_typed("memory/search", request).await
    }

    async fn get_memory(&mut self, request: MemoryGetRequest) -> TalosResult<MemoryGetResponse> {
        self.request_typed("memory/get", request).await
    }

    async fn workspace_info(
        &mut self,
        request: WorkspaceInfoRequest,
    ) -> TalosResult<WorkspaceInfoResponse> {
        self.request_typed("workspace/info", request).await
    }

    async fn list_workspace_docs(
        &mut self,
        request: WorkspaceDocListRequest,
    ) -> TalosResult<WorkspaceDocListResponse> {
        self.request_typed("workspace/doc/list", request).await
    }

    async fn read_workspace_doc(
        &mut self,
        request: WorkspaceDocReadRequest,
    ) -> TalosResult<WorkspaceDocReadResponse> {
        self.request_typed("workspace/doc/read", request).await
    }

    async fn write_workspace_doc(
        &mut self,
        request: WorkspaceDocWriteRequest,
    ) -> TalosResult<WorkspaceDocWriteResponse> {
        self.request_typed("workspace/doc/write", request).await
    }

    async fn shutdown(&mut self) -> TalosResult<()> {
        self.shutdown().await
    }
}
