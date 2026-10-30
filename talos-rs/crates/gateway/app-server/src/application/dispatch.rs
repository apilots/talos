//! Typed JSON-RPC request dispatch through the existing Gateway owner.
use super::*;

impl TalosApplicationAdapter {
    pub(super) async fn dispatch_request(
        &self,
        actor: &Actor,
        request: ClientRequest,
    ) -> ApplicationResult<ApplicationResponse> {
        match request.params {
            ClientRequestParams::Initialize(_) => Err(ApiError::new(ErrorCode::InvalidRequest)),
            ClientRequestParams::ProfileRead(params) => to_value(
                self.gateway
                    .read_profile(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::TrajectoryCapabilities(_params) => to_value(
                self.gateway
                    .trajectory_capabilities(actor)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::TrajectoryReadTurn(params) => to_value(
                self.gateway
                    .read_turn_trajectory(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::TrajectoryReadRequest(params) => to_value(
                self.gateway
                    .read_request_trajectory(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::TrajectoryReadTool(params) => to_value(
                self.gateway
                    .read_tool_trajectory(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::TrajectoryReadPayload(params) => to_value(
                self.gateway
                    .read_trajectory_payload(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::SessionList(params) => {
                to_value(self.list_sessions(actor, params).await?)
            }
            ClientRequestParams::SessionGet(params) => to_value(
                self.gateway
                    .get_session(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::SessionPatch(params) => {
                to_value(self.patch_session(actor, params).await?)
            }
            ClientRequestParams::SessionDelete(params) => {
                let response = self
                    .gateway
                    .delete_session(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?;
                to_value(response)
            }
            ClientRequestParams::SessionClose(params) => {
                let response = self
                    .gateway
                    .close_session(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?;
                to_value(response)
            }
            ClientRequestParams::SessionCompact(params) => to_value(
                self.gateway
                    .compact_session(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::SessionClear(params) => to_value(
                self.gateway
                    .clear_session(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::SessionActiveTurn(params) => {
                let snapshot = self
                    .gateway
                    .get_active_turn(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?;
                to_value(snapshot)
            }
            ClientRequestParams::SessionHistory(params) => to_value(
                self.gateway
                    .get_session_history(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::TimelineRecover(params) => {
                let snapshot = self
                    .gateway
                    .recover_timeline(
                        actor,
                        RecoverTimelineRequest {
                            session_id: params.session_id.clone(),
                            after_event_id: params.after_event_id,
                            replay_limit: Some(
                                params.replay_limit.unwrap_or(self.replay_limit).max(1),
                            ),
                        },
                    )
                    .await
                    .map_err(talos_error_to_api_error)?;
                to_value(snapshot)
            }
            ClientRequestParams::DesktopStreamOpen(_)
            | ClientRequestParams::DesktopStreamSend(_)
            | ClientRequestParams::DesktopStreamClose(_)
            | ClientRequestParams::DesktopWorkerIdentity(_)
            | ClientRequestParams::DesktopWorkerEnroll(_) => {
                Err(ApiError::new(ErrorCode::InvalidRequest))
            }
            ClientRequestParams::SyncMetrics(_) => to_value(
                self.app
                    .sync_metrics(actor)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::SyncStatus(_) => to_value(
                self.app
                    .sync_status(actor)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::SyncEnable(params) => to_value(
                self.app
                    .enable_sync(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::SyncDisable(_) => to_value(
                self.app
                    .disable_sync(actor)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::ComputerList(params) => to_value(
                self.gateway
                    .list_computers(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::ComputerRead(params) => to_value(
                self.gateway
                    .read_computer(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::ComputerCreate(params) => to_value(
                self.gateway
                    .create_computer(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::ComputerRename(params) => to_value(
                self.gateway
                    .rename_computer(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::ComputerDelete(params) => to_value(
                self.gateway
                    .delete_computer(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::ComputerSetStatus(params) => to_value(
                self.gateway
                    .set_computer_status(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::ComputerRotateCredential(params) => to_value(
                self.gateway
                    .rotate_computer_credential(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::ProjectCreate(params) => to_value(
                self.gateway
                    .create_project(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::ProjectList(_) => {
                let response = self
                    .gateway
                    .list_projects(actor, talos_types::ProjectListRequest::default())
                    .await
                    .map_err(talos_error_to_api_error)?;
                to_value(response)
            }
            ClientRequestParams::ProjectRead(params) => to_value(
                self.gateway
                    .get_project(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::ProjectPatch(params) => to_value(
                self.gateway
                    .patch_project(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::ProjectDelete(params) => {
                let response = self
                    .gateway
                    .delete_project(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?;
                to_value(response)
            }
            ClientRequestParams::FsDirectoryRead(params) => to_value(
                self.gateway
                    .read_fs_directory(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::FsDirectoryCreate(params) => to_value(
                self.gateway
                    .create_fs_directory(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::TurnStart(_) => Err(ApiError::new(ErrorCode::InvalidRequest)),
            ClientRequestParams::TurnInterrupt(params) => to_value(
                self.gateway
                    .interrupt_turn(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::TurnInput(params) => to_value(
                self.gateway
                    .submit_turn_input(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::TurnRetry(params) => {
                let request: SharedRetryTurnRequest = params;
                to_value(
                    self.gateway
                        .retry_turn(actor, request)
                        .await
                        .map_err(talos_error_to_api_error)?,
                )
            }
            ClientRequestParams::MessageQueueAdd(params) => to_value(
                self.gateway
                    .add_queued_message(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::MessageQueueList(params) => to_value(
                self.gateway
                    .list_message_queue(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::MessageQueueUpdate(params) => to_value(
                self.gateway
                    .update_queued_message(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::MessageQueueDelete(params) => to_value(
                self.gateway
                    .delete_queued_message(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::MessageQueueReorder(params) => to_value(
                self.gateway
                    .reorder_message_queue(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::MessageQueueStart(params) => to_value(
                self.gateway
                    .start_queued_message(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::EventSubscribe(_) => Err(ApiError::new(ErrorCode::InvalidRequest)),
            ClientRequestParams::EventUnsubscribe(_) => {
                Err(ApiError::new(ErrorCode::InvalidRequest))
            }
            ClientRequestParams::ModelList(params) => {
                let models = self
                    .gateway
                    .list_models(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?;
                to_value(models)
            }
            ClientRequestParams::ConfigGet(params) => to_value(
                self.gateway
                    .get_config(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::FileUpload(params) => {
                let FileUploadParams {
                    request,
                    data_base64,
                } = params;
                let data = decode_file_upload_data(
                    &data_base64,
                    self.app.files.limits().max_upload_bytes,
                )?;
                let body = Box::pin(futures_util::stream::iter([Ok(data)]));
                to_value(
                    self.gateway
                        .upload_file(actor, request, body)
                        .await
                        .map_err(talos_error_to_api_error)?,
                )
            }
            ClientRequestParams::FileList(params) => to_value(
                self.gateway
                    .list_files(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::FileRead(params) => to_value(
                self.gateway
                    .read_file(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::FileContent(params) => to_value(
                self.gateway
                    .read_file_content(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::FilePreviewPrepare(params) => to_value(
                self.gateway
                    .prepare_file_preview(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::FilePreviewRead(params) => to_value(
                self.gateway
                    .read_file_preview(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::FilePreviewClose(params) => to_value(
                self.gateway
                    .close_file_preview(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::FileUpdate(params) => to_value(
                self.gateway
                    .update_file(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::FileDelete(params) => to_value(
                self.gateway
                    .delete_file(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::MemoryFileList(params) => to_value(
                self.gateway
                    .list_memory_files(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::MemoryFileRead(params) => to_value(
                self.gateway
                    .read_memory_file(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::MemoryFileWrite(params) => to_value(
                self.gateway
                    .write_memory_file(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::WorkspaceInfo(params) => to_value(
                self.gateway
                    .workspace_info(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::WorkspaceDocList(params) => to_value(
                self.gateway
                    .list_workspace_docs(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::WorkspaceDocRead(params) => to_value(
                self.gateway
                    .read_workspace_doc(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::WorkspaceDocWrite(params) => to_value(
                self.gateway
                    .write_workspace_doc(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::ConfigUpdate(params) => to_value(
                self.gateway
                    .update_config(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::ConfigDelete(params) => to_value(
                self.gateway
                    .delete_config(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::ConfigReload(params) => to_value(
                self.gateway
                    .reload_config(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::SkillsList(params) => to_value(
                self.gateway
                    .list_skills(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::SkillsRefresh(params) => to_value(
                self.gateway
                    .refresh_skills(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::SkillsSuggest(params) => to_value(
                self.gateway
                    .suggest_skills(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::SkillsRead(params) => to_value(
                self.gateway
                    .read_skill(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::SkillsResolve(params) => to_value(
                self.gateway
                    .resolve_skills(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::PluginSourceList(params) => to_value(
                self.gateway
                    .list_plugin_sources(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::PluginSourceAdd(params) => to_value(
                self.gateway
                    .add_plugin_source(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::PluginSourceRefresh(params) => to_value(
                self.gateway
                    .refresh_plugin_source(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::PluginSourceRemove(params) => to_value(
                self.gateway
                    .remove_plugin_source(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::PluginSearch(params) => to_value(
                self.gateway
                    .search_plugins(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::McpServerImport(params) => to_value(
                self.gateway
                    .import_mcp_server(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::McpAuthenticationStart(params) => to_value(
                self.gateway
                    .start_mcp_authentication(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::McpAuthenticationRead(params) => to_value(
                self.gateway
                    .read_mcp_authentication(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::McpCredentialsUpdate(params) => to_value(
                self.gateway
                    .update_mcp_credentials(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::DesktopOpen(params) => to_value(
                self.gateway
                    .open_desktop(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::Browser(params) => to_value(
                self.gateway
                    .browser(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::McpServerList(params) => to_value(
                self.gateway
                    .list_mcp_servers(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::PluginList(params) => to_value(
                self.gateway
                    .list_plugins(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::PluginRead(params) => to_value(
                self.gateway
                    .read_plugin(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::PluginInstall(params) => to_value(
                self.gateway
                    .install_plugin(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::PluginUpdate(params) => to_value(
                self.gateway
                    .update_plugin(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::PluginUninstall(params) => to_value(
                self.gateway
                    .uninstall_plugin(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::PluginEnable(params) => to_value(
                self.gateway
                    .enable_plugin(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::PluginDisable(params) => to_value(
                self.gateway
                    .disable_plugin(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::AuthLogout(params) => {
                let response = self
                    .gateway
                    .logout(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?;
                to_value(response)
            }
            ClientRequestParams::AuthLoginStart(params) => to_value(
                self.gateway
                    .start_cloud_login(actor, params, talos_auth::CloudLoginPersistence::LocalCache)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::AuthLoginStatus(params) => to_value(
                self.gateway
                    .cloud_login_status(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::AuthLoginCancel(params) => {
                let response = self
                    .gateway
                    .cancel_cloud_login(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?;
                to_value(response)
            }
            ClientRequestParams::AuthSessionRead(params) => to_value(
                self.gateway
                    .read_auth_session(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::AuthSessionClear(params) => {
                let response = self
                    .gateway
                    .clear_auth_session(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?;
                to_value(response)
            }
            ClientRequestParams::UserCurrent(params) => to_value(
                self.gateway
                    .current_user(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::MemorySearch(params) => to_value(
                self.gateway
                    .search_memory(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
            ClientRequestParams::MemoryGet(params) => to_value(
                self.gateway
                    .get_memory(actor, params)
                    .await
                    .map_err(talos_error_to_api_error)?,
            ),
        }
    }
}
