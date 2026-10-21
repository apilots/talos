//! Desktop identity and enrollment over the existing management runtime.
use async_trait::async_trait;
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use talos_app_protocol::requests::{ClientRequest, ClientRequestParams};
use talos_app_server::AppServerApplication;
use talos_app_server::{ApplicationResponse, ApplicationResult};
use talos_auth::AuthService;
use talos_config::{Config, YamlConfigStore};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::{AccountService, Actor, TimelineSubscription};
use talos_types::{DesktopWorkerIdentityResponse, StartTurnRequest, SubscribeTimelineRequest};
use tokio::sync::Mutex;

pub(crate) struct DesktopApplication {
    service: Arc<talos_service::TalosService>,
    media: crate::desktop_media::DesktopMedia,
    pub(crate) config: Config,
    pub(crate) store: YamlConfigStore,
    pub(crate) account: AccountService,
    pub(crate) auth: Arc<dyn AuthService>,
    pub(crate) worker: Arc<Mutex<Option<talos_worker::RegisteredWorker>>>,
    pub(crate) execution: talos_worker::TalosWorker,
    pub(crate) rpc: Arc<talos_worker::WorkerRpcHost>,
    pub(crate) device_name: String,
    pub(crate) worker_failures: tokio::sync::watch::Sender<Option<talos_error::ApiError>>,
    local: Arc<dyn AppServerApplication>,
    pub(crate) sync_host: Arc<crate::application_sync::ApplicationSyncHost>,
}

impl DesktopApplication {
    pub(crate) fn new(
        runtime: &crate::TalosApplicationRuntime,
        device_name: String,
        local: Arc<dyn AppServerApplication>,
        host: Arc<talos_app_server::DesktopHostBridge>,
    ) -> Self {
        Self {
            service: runtime.service.clone(),
            media: crate::desktop_media::DesktopMedia::new(host),
            sync_host: runtime.sync_host.clone(),
            store: YamlConfigStore::new(runtime.config.source_path()),
            config: runtime.config.clone(),
            account: runtime.service.account.clone(),
            auth: runtime.auth_service.clone(),
            worker: runtime.worker_registration.clone(),
            execution: runtime.worker.clone(),
            rpc: runtime.worker_rpc.clone(),
            device_name,
            worker_failures: runtime.worker_failures.clone(),
            local,
        }
    }

    pub(crate) async fn stop_registration(&self) -> TalosResult<()> {
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
            ClientRequestParams::DesktopStreamOpen(params) => {
                value(self.media.open(&self.service, actor, params).await?)
            }
            ClientRequestParams::DesktopStreamSend(params) => {
                value(self.media.send(actor, params).await?)
            }
            ClientRequestParams::DesktopStreamClose(params) => {
                value(self.media.close(actor, params).await?)
            }
            ClientRequestParams::DesktopWorkerIdentity(_) => value(self.worker_identity().await?),
            ClientRequestParams::DesktopWorkerEnroll(params) => {
                if actor.access_token.is_some()
                    && self
                        .account
                        .read_auth_session(actor, Default::default())
                        .map_err(project_error)?
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
            _ => Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Unknown Desktop host method",
            )),
        }
    }

    async fn worker_identity(&self) -> TalosResult<DesktopWorkerIdentityResponse> {
        Ok(DesktopWorkerIdentityResponse {
            node_id: Some(self.sync_host.local_computer_id().await?),
            control_plane_endpoint: Some(self.config.cloud.agent_base_url.clone()),
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
        if let ClientRequestParams::Browser(params) = &request.params {
            if matches!(params.operation, talos_types::BrowserOperation::OpenLocal) {
                let identity = self.worker_identity().await.map_err(project_error)?;
                if params.node_id.as_deref() != identity.node_id.as_deref() {
                    return Err(project_error(TalosError::localized(
                        talos_error::ErrorLocalization::BrowserCapabilityUnavailable {
                            operation: talos_error::BrowserCapabilityOperation::OpenLocal,
                            reason: talos_error::BrowserCapabilityReason::ClientMismatch,
                        },
                    )));
                }
                // Verify the same local Worker through the regular actor-scoped Service read.
                self.local
                    .dispatch(
                        actor,
                        ClientRequest {
                            method: talos_app_protocol::ClientMethod::Browser,
                            params: ClientRequestParams::Browser(talos_types::BrowserRequest {
                                node_id: params.node_id.clone(),
                                operation: talos_types::BrowserOperation::Read,
                            }),
                        },
                    )
                    .await?;
                let snapshot = self
                    .execution
                    .browser()
                    .operate(
                        actor.user_id.clone(),
                        talos_types::BrowserOperation::OpenLocal,
                    )
                    .await
                    .map_err(project_error)?;
                return Ok(ApplicationResponse {
                    body: value(snapshot).map_err(project_error)?,
                    started_turn: None,
                });
            }
        }
        let host_method = matches!(
            request.params,
            ClientRequestParams::DesktopStreamOpen(_)
                | ClientRequestParams::DesktopStreamSend(_)
                | ClientRequestParams::DesktopStreamClose(_)
                | ClientRequestParams::DesktopWorkerIdentity(_)
                | ClientRequestParams::DesktopWorkerEnroll(_)
        );
        if !host_method {
            if matches!(
                request.params,
                ClientRequestParams::AuthLogout(_) | ClientRequestParams::AuthSessionClear(_)
            ) {
                // Enrollment may hold the Worker lock while G is unavailable.
                // Cancel its account owner before taking that lock for logout.
                let account = if matches!(request.params, ClientRequestParams::AuthSessionClear(_))
                {
                    self.account
                        .read_auth_session(actor, Default::default())
                        .map_err(project_error)?
                        .session
                        .map(|session| Actor::regular(session.user.user_id))
                } else {
                    Some(actor.clone())
                };
                if let Some(account) = account {
                    self.service
                        .suspend_account_sync(&account)
                        .await
                        .map_err(project_error)?;
                }
                self.media.close_all().await;
                let browser = self.execution.browser().reset_account(&actor.user_id).await;
                let cleanup = self.stop_registration().await;
                let cleanup = browser.and(cleanup);
                let result = self.local.dispatch(actor, request).await;
                cleanup.map_err(project_error)?;
                return result;
            }
            return self.local.dispatch(actor, request).await;
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
        self.local.start_turn(actor, source, request).await
    }

    async fn subscribe_timeline_events(
        &self,
        actor: &Actor,
        params: SubscribeTimelineRequest,
    ) -> ApplicationResult<TimelineSubscription> {
        self.local.subscribe_timeline_events(actor, params).await
    }

    async fn cancel_active_turn(
        &self,
        actor: &Actor,
        session_id: &str,
        turn_id: &str,
    ) -> ApplicationResult<()> {
        self.local
            .cancel_active_turn(actor, session_id, turn_id)
            .await
    }

    async fn drain_background_tasks(&self, timeout: Duration) -> TalosResult<()> {
        self.sync_host.shutdown_refreshes().await;
        self.media.close_all().await;
        self.stop_registration().await?;
        self.local.drain_background_tasks(timeout).await
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

fn project_error(error: TalosError) -> talos_error::ApiError {
    talos_logging::report_error(&error);
    error.to_api_error()
}
