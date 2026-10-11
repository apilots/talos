//! Desktop host composition. Cloud mode never constructs product Stores or a local Coordinator.
use async_trait::async_trait;
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use talos_app_protocol::requests::{ClientRequest, ClientRequestParams};
use talos_app_server::AppServerApplication;
use talos_app_server::{ApplicationResponse, ApplicationResult};
use talos_auth::{AuthService, CloudLoginPersistence};
use talos_config::{Config, DesktopConnectionMode, YamlConfigStore};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::{AccountService, Actor, TimelineSubscription};
use talos_types::{
    DesktopConnectionResponse, DesktopWorkerIdentityResponse, StartTurnRequest,
    SubscribeTimelineRequest,
};
use tokio::sync::Mutex;

pub(crate) struct DesktopApplication {
    pub(crate) config: Config,
    pub(crate) store: YamlConfigStore,
    pub(crate) account: AccountService,
    pub(crate) auth: Arc<dyn AuthService>,
    pub(crate) worker: Mutex<Option<talos_worker::RegisteredWorker>>,
    pub(crate) device_name: String,
    pub(crate) worker_failures: tokio::sync::watch::Sender<Option<talos_error::ApiError>>,
    local: Option<Arc<dyn AppServerApplication>>,
}

impl DesktopApplication {
    pub(crate) fn new(
        config: Config,
        account: AccountService,
        auth: Arc<dyn AuthService>,
        device_name: String,
        local: Option<Arc<dyn AppServerApplication>>,
    ) -> Self {
        Self {
            store: YamlConfigStore::new(config.source_path()),
            config,
            account,
            auth,
            worker: Mutex::new(None),
            device_name,
            worker_failures: tokio::sync::watch::channel(None).0,
            local,
        }
    }

    pub(crate) async fn stop_worker(&self) -> TalosResult<()> {
        if let Some(worker) = self.worker.lock().await.take() {
            worker.shutdown().await?;
        }
        Ok(())
    }

    async fn dispatch_host(
        &self,
        actor: &Actor,
        request: ClientRequest,
    ) -> TalosResult<serde_json::Value> {
        match request.params {
            ClientRequestParams::DesktopConnectionRead(_) => value(DesktopConnectionResponse {
                connection_mode: self.store.load_desktop()?.connection_mode,
                control_plane_endpoint: self.config.cloud.agent_base_url.clone(),
            }),
            ClientRequestParams::DesktopConnectionSet(params) => {
                self.stop_worker().await?;
                self.store.save_desktop_connection(params.connection_mode)?;
                value(DesktopConnectionResponse {
                    connection_mode: params.connection_mode,
                    control_plane_endpoint: self.config.cloud.agent_base_url.clone(),
                })
            }
            ClientRequestParams::DesktopWorkerIdentity(_) => value(self.worker_identity()?),
            ClientRequestParams::DesktopWorkerEnroll(params) => {
                if actor.access_token.is_some()
                    && self
                        .account
                        .read_auth_session(actor, Default::default())?
                        .session
                        .is_none()
                {
                    return Err(TalosError::new(
                        ErrorCode::Unauthenticated,
                        "Enrollment credentials do not match the Desktop login",
                    ));
                }
                value(self.enroll_worker(params.node_id).await?)
            }
            ClientRequestParams::AuthLoginStart(params) => value(
                self.account
                    .start_cloud_login(params, CloudLoginPersistence::LocalCache)
                    .await?,
            ),
            ClientRequestParams::AuthLoginStatus(params) => {
                value(self.account.cloud_login_status(params).await?)
            }
            ClientRequestParams::AuthLoginCancel(params) => {
                self.account.cancel_cloud_login(params).await?;
                value(talos_types::CloudLoginCancelResponse { success: true })
            }
            ClientRequestParams::AuthSessionRead(params) => {
                value(self.account.read_auth_session(actor, params)?)
            }
            ClientRequestParams::AuthSessionClear(params) => {
                let cleanup = self.stop_worker().await;
                self.account.clear_auth_session(params)?;
                cleanup?;
                value(talos_types::AuthSessionClearResponse { success: true })
            }
            ClientRequestParams::AuthLogout(params) => {
                let cleanup = self.stop_worker().await;
                self.account.logout_with_request(actor, params).await?;
                cleanup?;
                value(talos_types::AuthLogoutResponse { success: true })
            }
            ClientRequestParams::UserCurrent(params) => value(talos_types::UserCurrentResponse {
                user: self
                    .account
                    .current_user_with_request(actor, params)
                    .await?,
            }),
            _ => Err(cloud_business_unavailable()),
        }
    }

    fn worker_identity(&self) -> TalosResult<DesktopWorkerIdentityResponse> {
        if self.config.desktop.connection_mode == DesktopConnectionMode::Local {
            return Ok(DesktopWorkerIdentityResponse {
                node_id: Some("embedded".into()),
                control_plane_endpoint: None,
            });
        }
        let runner = self.store.load_agent_node()?.runner;
        Ok(DesktopWorkerIdentityResponse {
            node_id: runner.as_ref().map(|runner| runner.node_id.clone()),
            control_plane_endpoint: runner.map(|runner| runner.control_plane_endpoint),
        })
    }
}

#[async_trait]
impl AppServerApplication for DesktopApplication {
    fn subscribe_worker_failures(
        &self,
    ) -> Option<tokio::sync::watch::Receiver<Option<talos_error::ApiError>>> {
        Some(self.worker_failures.subscribe())
    }

    async fn dispatch(
        &self,
        actor: &Actor,
        request: ClientRequest,
    ) -> ApplicationResult<ApplicationResponse> {
        let host_method = matches!(
            request.params,
            ClientRequestParams::DesktopConnectionRead(_)
                | ClientRequestParams::DesktopConnectionSet(_)
                | ClientRequestParams::DesktopWorkerIdentity(_)
                | ClientRequestParams::DesktopWorkerEnroll(_)
        );
        if !host_method {
            if let Some(local) = &self.local {
                return local.dispatch(actor, request).await;
            }
        }
        self.dispatch_host(actor, request)
            .await
            .map(|body| ApplicationResponse {
                body,
                started_turn: None,
            })
            .map_err(project_error)
    }

    async fn start_turn(
        &self,
        actor: &Actor,
        source: talos_types::talos::SessionSource,
        request: StartTurnRequest,
    ) -> ApplicationResult<ApplicationResponse> {
        if let Some(local) = &self.local {
            return local.start_turn(actor, source, request).await;
        }
        Err(project_error(cloud_business_unavailable()))
    }

    async fn subscribe_timeline_events(
        &self,
        actor: &Actor,
        params: SubscribeTimelineRequest,
    ) -> ApplicationResult<TimelineSubscription> {
        if let Some(local) = &self.local {
            return local.subscribe_timeline_events(actor, params).await;
        }
        Err(project_error(cloud_business_unavailable()))
    }

    async fn cancel_active_turn(
        &self,
        actor: &Actor,
        session_id: &str,
        turn_id: &str,
    ) -> ApplicationResult<()> {
        if let Some(local) = &self.local {
            return local.cancel_active_turn(actor, session_id, turn_id).await;
        }
        Err(project_error(cloud_business_unavailable()))
    }

    async fn drain_background_tasks(&self, timeout: Duration) -> TalosResult<()> {
        self.stop_worker().await?;
        if let Some(local) = &self.local {
            local.drain_background_tasks(timeout).await?;
        }
        Ok(())
    }
}

fn value<T: Serialize>(value: T) -> TalosResult<serde_json::Value> {
    serde_json::to_value(value).map_err(|error| {
        TalosError::internal_with_source(
            "Serialize Desktop host response; technical details were withheld.",
            error,
        )
    })
}

fn cloud_business_unavailable() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Cloud product operations must use the cloud product SDK",
    )
}

fn project_error(error: TalosError) -> talos_error::ApiError {
    talos_logging::report_error(&error);
    error.to_api_error()
}
