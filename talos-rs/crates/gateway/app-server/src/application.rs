mod codec;
use codec::{decode_file_upload_data, talos_error_to_api_error, to_value};

use async_trait::async_trait;
use serde_json::Value;
use talos_agent_api::AgentTurnRef;
use talos_app_protocol::errors::AppServerErrorBody;
use talos_app_protocol::requests::{ClientRequest, ClientRequestParams, FileUploadParams};
use talos_auth::CloudSessionCache;
use talos_gateway::{Gateway, ServiceGateway};
use talos_service::{Actor, TalosService, TimelineSubscription};
use talos_types::{
    RecoverTimelineRequest, RetryTurnRequest as SharedRetryTurnRequest, StartTurnRequest,
    SubscribeTimelineRequest,
};

use std::sync::Arc;
use std::time::Duration;
use talos_error::{ApiError, ErrorCode, TalosResult};

/// Abstraction for draining application-owned background tasks during shutdown.
#[async_trait::async_trait]
pub trait BackgroundTaskDrain: Send + Sync {
    async fn drain(&self, timeout: Duration) -> TalosResult<()>;
}

/// Blanket impl for closure-based drain providers.
#[async_trait::async_trait]
impl<F, Fut> BackgroundTaskDrain for F
where
    F: Fn(Duration) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = TalosResult<()>> + Send,
{
    async fn drain(&self, timeout: Duration) -> TalosResult<()> {
        self(timeout).await
    }
}

pub type ApplicationResult<T> = Result<T, ApiError>;
pub type ProtocolResult<T> = Result<T, AppServerErrorBody>;

/// Internal result of one typed application dispatch.
///
/// The body is already serialized for the JSON-RPC transport. `started_turn`
/// carries the canonical identity needed by connection-owned lifecycle control;
/// transports must not recover that identity by inspecting JSON fields.
pub struct ApplicationResponse {
    pub body: Value,
    pub started_turn: Option<AgentTurnRef>,
}

#[async_trait]
pub trait AppServerApplication: Send + Sync {
    /// Desktop-only terminal registration feedback; never a product event or durable state.
    fn subscribe_worker_failures(&self) -> Option<tokio::sync::watch::Receiver<Option<ApiError>>> {
        None
    }

    async fn dispatch(
        &self,
        actor: &Actor,
        request: ClientRequest,
    ) -> ApplicationResult<ApplicationResponse>;

    async fn start_turn(
        &self,
        actor: &Actor,
        source: talos_types::talos::SessionSource,
        request: StartTurnRequest,
    ) -> ApplicationResult<ApplicationResponse>;

    async fn subscribe_timeline_events(
        &self,
        actor: &Actor,
        params: SubscribeTimelineRequest,
    ) -> ApplicationResult<TimelineSubscription>;

    async fn cancel_active_turn(
        &self,
        actor: &Actor,
        session_id: &str,
        turn_id: &str,
    ) -> ApplicationResult<()>;

    async fn drain_background_tasks(&self, timeout: Duration) -> TalosResult<()>;
}

#[derive(Clone)]
pub struct TalosApplicationAdapter {
    app: std::sync::Arc<TalosService>,
    gateway: Arc<dyn Gateway>,
    replay_limit: usize,
    drain_provider: Arc<Option<Box<dyn BackgroundTaskDrain>>>,
}

impl TalosApplicationAdapter {
    pub fn new(app: std::sync::Arc<TalosService>, replay_limit: usize) -> Self {
        let gateway: Arc<dyn Gateway> = Arc::new(ServiceGateway::new(app.clone()));
        Self {
            gateway,
            app,
            replay_limit: replay_limit.max(1),
            drain_provider: Arc::new(None),
        }
    }

    pub fn with_drain_provider(mut self, drain: Box<dyn BackgroundTaskDrain>) -> Self {
        self.drain_provider = Arc::new(Some(drain));
        self
    }

    pub fn with_cloud_session_cache(self, cache: CloudSessionCache) -> Self {
        self.app
            .account
            .set_cloud_session_cache(cache)
            .expect("cloud session cache should configure");
        self
    }

    pub fn with_cloud_api_base_url(self, api_base_url: impl Into<String>) -> Self {
        self.app
            .account
            .set_cloud_api_base_url(api_base_url)
            .expect("cloud api base URL should configure");
        self
    }
}

#[async_trait]
impl AppServerApplication for TalosApplicationAdapter {
    async fn dispatch(
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
            ClientRequestParams::DesktopConnectionRead(_)
            | ClientRequestParams::DesktopConnectionSet(_)
            | ClientRequestParams::DesktopWorkerIdentity(_)
            | ClientRequestParams::DesktopWorkerEnroll(_) => {
                Err(ApiError::new(ErrorCode::InvalidRequest))
            }
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

    async fn subscribe_timeline_events(
        &self,
        actor: &Actor,
        mut request: SubscribeTimelineRequest,
    ) -> ApplicationResult<TimelineSubscription> {
        request.replay_limit = Some(request.replay_limit.unwrap_or(self.replay_limit).max(1));
        self.app
            .open_timeline_subscription(actor, request)
            .await
            .map_err(talos_error_to_api_error)
    }

    async fn cancel_active_turn(
        &self,
        actor: &Actor,
        session_id: &str,
        turn_id: &str,
    ) -> ApplicationResult<()> {
        self.gateway
            .interrupt_turn(
                actor,
                talos_agent_api::InterruptTurnRequest {
                    identity: talos_types::AgentTurnRef::new(session_id, turn_id),
                },
            )
            .await
            .map(|_| ())
            .map_err(talos_error_to_api_error)
    }

    async fn drain_background_tasks(&self, timeout: Duration) -> TalosResult<()> {
        if let Some(drain) = self.drain_provider.as_deref() {
            drain.drain(timeout).await
        } else {
            Ok(())
        }
    }

    async fn start_turn(
        &self,
        actor: &Actor,
        source: talos_types::talos::SessionSource,
        request: StartTurnRequest,
    ) -> ApplicationResult<ApplicationResponse> {
        let client_request_id = request.client_request_id.clone();
        let response = self
            .gateway
            .start_turn(actor, source, request, client_request_id)
            .await
            .map_err(talos_error_to_api_error)?;
        let started_turn = AgentTurnRef::new(&response.session_id, &response.turn_id);
        to_value(response).map(|body| ApplicationResponse {
            body: body.body,
            started_turn: Some(started_turn),
        })
    }
}

impl TalosApplicationAdapter {
    async fn list_sessions(
        &self,
        actor: &Actor,
        request: talos_types::SessionListRequest,
    ) -> ApplicationResult<talos_types::SessionListResponse> {
        self.gateway
            .list_sessions(actor, request)
            .await
            .map_err(talos_error_to_api_error)
    }

    async fn patch_session(
        &self,
        actor: &Actor,
        request: talos_types::SessionPatchRequest,
    ) -> ApplicationResult<talos_types::SessionUpdateResponse> {
        self.gateway
            .patch_session(actor, request)
            .await
            .map_err(talos_error_to_api_error)
    }
}

#[cfg(test)]
#[path = "application_tests.rs"]
mod tests;
