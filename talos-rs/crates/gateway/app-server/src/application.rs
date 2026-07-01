use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use talos_app_protocol::errors::AppServerErrorBody;
use talos_app_protocol::requests::{ClientRequest, ClientRequestParams};
use talos_auth::CloudSessionCache;
use talos_gateway::{Gateway, ServiceGateway};
use talos_service::{Actor, TalosService, TimelineSubscriptionSource};
use talos_types::{
    RecoverTimelineRequest, RetryTurnRequest as SharedRetryTurnRequest, StartTurnRequest,
    StartTurnResponse, SubscribeTimelineRequest,
};

use std::sync::Arc;
use std::time::Duration;
use talos_error::TalosResult;

/// Abstraction for draining background tasks (e.g. memory flush) during shutdown.
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

pub type ProtocolResult<T> = Result<T, AppServerErrorBody>;

#[async_trait]
pub trait AppServerApplication: Send + Sync {
    async fn dispatch(&self, actor: &Actor, request: ClientRequest) -> ProtocolResult<Value>;

    async fn subscribe_timeline_events(
        &self,
        actor: &Actor,
        params: SubscribeTimelineRequest,
    ) -> ProtocolResult<SubscriptionSource>;

    async fn cancel_active_turn(&self, actor: &Actor, session_id: &str) -> ProtocolResult<()>;

    async fn drain_background_tasks(&self, timeout: Duration) -> TalosResult<()>;
}

pub type SubscriptionSource = TimelineSubscriptionSource;

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
    async fn dispatch(&self, actor: &Actor, request: ClientRequest) -> ProtocolResult<Value> {
        match request.params {
            ClientRequestParams::Initialize(_) => Err(AppServerErrorBody::invalid_request(
                "initialize is handled by the app-server runtime",
            )),
            ClientRequestParams::SessionList(params) => {
                to_value(self.list_sessions(actor, params).await?)
            }
            ClientRequestParams::SessionGet(params) => to_value(
                self.gateway
                    .get_session(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::SessionPatch(params) => {
                to_value(self.patch_session(actor, params).await?)
            }
            ClientRequestParams::SessionDelete(params) => {
                let response = self
                    .gateway
                    .delete_session(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(response)
            }
            ClientRequestParams::SessionClose(params) => {
                let response = self
                    .gateway
                    .close_session(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(response)
            }
            ClientRequestParams::SessionCompact(params) => to_value(
                self.gateway
                    .compact_session(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::SessionClear(params) => to_value(
                self.gateway
                    .clear_session(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::SessionActiveTurn(params) => {
                let snapshot = self
                    .gateway
                    .get_active_turn(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(snapshot)
            }
            ClientRequestParams::SessionHistory(params) => to_value(
                self.gateway
                    .get_session_history(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
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
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(snapshot)
            }
            ClientRequestParams::ProjectCreate(params) => to_value(
                self.gateway
                    .create_project(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::ProjectList(_) => {
                let response = self
                    .gateway
                    .list_projects(actor, talos_types::ProjectListRequest::default())
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(response)
            }
            ClientRequestParams::ProjectRead(params) => to_value(
                self.gateway
                    .get_project(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::ProjectPatch(params) => to_value(
                self.gateway
                    .patch_project(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::ProjectDelete(params) => {
                let response = self
                    .gateway
                    .delete_project(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(response)
            }
            ClientRequestParams::FsDirectoryRead(params) => to_value(
                self.gateway
                    .read_fs_directory(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::FsDirectoryCreate(params) => to_value(
                self.gateway
                    .create_fs_directory(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::TurnStart(params) => {
                to_value(self.start_turn(actor, params.into()).await?)
            }
            ClientRequestParams::TurnInterrupt(params) => to_value(
                self.gateway
                    .interrupt_turn(actor, params.into())
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::TurnRetry(params) => {
                let request: SharedRetryTurnRequest = params.into();
                to_value(
                    self.gateway
                        .retry_turn(actor, request)
                        .await
                        .map_err(AppServerErrorBody::from_talos_error)?,
                )
            }
            ClientRequestParams::EventSubscribe(_) => Err(AppServerErrorBody::invalid_request(
                "event.subscribe is handled by the app-server runtime",
            )),
            ClientRequestParams::EventUnsubscribe(_) => Err(AppServerErrorBody::invalid_request(
                "event.unsubscribe is handled by the app-server runtime",
            )),
            ClientRequestParams::ToolDynamicResultSubmit(params) => to_value(
                self.gateway
                    .submit_dynamic_tool_result(actor, params.into())
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::ModelList(_) => {
                let models = self
                    .gateway
                    .list_models(actor, talos_types::ModelListRequest::default())
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(models)
            }
            ClientRequestParams::ConfigGet(params) => to_value(
                self.gateway
                    .get_config(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::MemoryFileList(params) => to_value(
                self.gateway
                    .list_memory_files(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::MemoryFileRead(params) => to_value(
                self.gateway
                    .read_memory_file(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::MemoryFileWrite(params) => to_value(
                self.gateway
                    .write_memory_file(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::WorkspaceDocList(params) => to_value(
                self.gateway
                    .list_workspace_docs(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::WorkspaceDocRead(params) => to_value(
                self.gateway
                    .read_workspace_doc(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::WorkspaceDocWrite(params) => to_value(
                self.gateway
                    .write_workspace_doc(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::WorkspaceInfo(params) => to_value(
                self.gateway
                    .workspace_info(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::ConfigUpdate(params) => to_value(
                self.gateway
                    .update_config(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::ConfigDelete(params) => to_value(
                self.gateway
                    .delete_config(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::ConfigReload(params) => to_value(
                self.gateway
                    .reload_config(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::SkillsList(params) => to_value(
                self.gateway
                    .list_skills(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::SkillsRefresh(params) => to_value(
                self.gateway
                    .refresh_skills(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::SkillsSuggest(params) => to_value(
                self.gateway
                    .suggest_skills(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::SkillsRead(params) => to_value(
                self.gateway
                    .read_skill(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::SkillsResolve(params) => to_value(
                self.gateway
                    .resolve_skills(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::PluginSourceList(params) => to_value(
                self.gateway
                    .list_plugin_sources(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::PluginSourceAdd(params) => to_value(
                self.gateway
                    .add_plugin_source(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::PluginSourceRefresh(params) => to_value(
                self.gateway
                    .refresh_plugin_source(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::PluginSourceRemove(params) => to_value(
                self.gateway
                    .remove_plugin_source(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::PluginSearch(params) => to_value(
                self.gateway
                    .search_plugins(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::PluginList(params) => to_value(
                self.gateway
                    .list_plugins(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::PluginRead(params) => to_value(
                self.gateway
                    .read_plugin(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::PluginInstall(params) => to_value(
                self.gateway
                    .install_plugin(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::PluginUpdate(params) => to_value(
                self.gateway
                    .update_plugin(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::PluginUninstall(params) => to_value(
                self.gateway
                    .uninstall_plugin(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::PluginEnable(params) => to_value(
                self.gateway
                    .enable_plugin(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::PluginDisable(params) => to_value(
                self.gateway
                    .disable_plugin(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::AuthLogout(params) => {
                let response = self
                    .gateway
                    .logout(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(response)
            }
            ClientRequestParams::AuthLoginStart(params) => to_value(
                self.gateway
                    .start_cloud_login(actor, params, talos_auth::CloudLoginPersistence::LocalCache)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::AuthLoginStatus(params) => to_value(
                self.gateway
                    .cloud_login_status(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::AuthLoginCancel(params) => {
                let response = self
                    .gateway
                    .cancel_cloud_login(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(response)
            }
            ClientRequestParams::AuthSessionRead(params) => to_value(
                self.gateway
                    .read_auth_session(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::AuthSessionClear(params) => {
                let response = self
                    .gateway
                    .clear_auth_session(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?;
                to_value(response)
            }
            ClientRequestParams::UserCurrent(params) => to_value(
                self.gateway
                    .current_user(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
            ClientRequestParams::MemorySearch(params) => to_value(
                self.gateway
                    .search_memory(actor, params)
                    .await
                    .map_err(AppServerErrorBody::from_talos_error)?,
            ),
        }
    }

    async fn subscribe_timeline_events(
        &self,
        actor: &Actor,
        mut request: SubscribeTimelineRequest,
    ) -> ProtocolResult<SubscriptionSource> {
        request.replay_limit = Some(request.replay_limit.unwrap_or(self.replay_limit).max(1));
        self.app
            .timeline_subscription_source(actor, request)
            .await
            .map_err(AppServerErrorBody::from_talos_error)
    }

    async fn cancel_active_turn(&self, actor: &Actor, session_id: &str) -> ProtocolResult<()> {
        self.gateway
            .interrupt_turn(
                actor,
                talos_types::InterruptTurnRequest {
                    session_id: session_id.to_string(),
                },
            )
            .await
            .map(|_| ())
            .map_err(AppServerErrorBody::from_talos_error)
    }

    async fn drain_background_tasks(&self, timeout: Duration) -> TalosResult<()> {
        if let Some(drain) = self.drain_provider.as_deref() {
            drain.drain(timeout).await
        } else {
            Ok(())
        }
    }
}

impl TalosApplicationAdapter {
    async fn list_sessions(
        &self,
        actor: &Actor,
        request: talos_types::SessionListRequest,
    ) -> ProtocolResult<talos_types::SessionListResponse> {
        self.gateway
            .list_sessions(actor, request)
            .await
            .map_err(AppServerErrorBody::from_talos_error)
    }

    async fn patch_session(
        &self,
        actor: &Actor,
        request: talos_types::SessionPatchRequest,
    ) -> ProtocolResult<talos_types::SessionUpdateResponse> {
        self.gateway
            .patch_session(actor, request)
            .await
            .map_err(AppServerErrorBody::from_talos_error)
    }

    async fn start_turn(
        &self,
        actor: &Actor,
        request: StartTurnRequest,
    ) -> ProtocolResult<StartTurnResponse> {
        let client_request_id = request.client_request_id.clone();
        self.gateway
            .start_turn(actor, request, client_request_id)
            .await
            .map_err(AppServerErrorBody::from_talos_error)
    }
}

fn to_value<T: Serialize>(value: T) -> ProtocolResult<Value> {
    serde_json::to_value(value)
        .map_err(|error| AppServerErrorBody::invalid_request(error.to_string()))
}
