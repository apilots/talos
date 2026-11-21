//! Desktop identity and enrollment over the existing management runtime.
use super::desktop_cloud::{ChatAttachment, CloudCall, DesktopCloud};
use async_trait::async_trait;
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use talos_app_protocol::requests::{ClientRequest, ClientRequestParams};
use talos_app_server::AppServerApplication;
use talos_app_server::{ApplicationResponse, ApplicationResult};
use talos_auth::AuthService;
use talos_config::{Config, YamlConfigStore};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::{AccountService, Actor};
use talos_types::{DesktopWorkerIdentityResponse, StartTurnRequest, SubscribeTimelineRequest};
use tokio::sync::Mutex;

enum ConversationOperation<'a> {
    Read,
    Timeline,
    Command,
    StartTurn(Option<&'a str>),
}

pub(crate) struct DesktopApplication {
    service: Arc<talos_service::TalosService>,
    media: crate::desktop_media::DesktopMedia,
    account_change: Mutex<()>,
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
    cloud: tokio::sync::OnceCell<DesktopCloud>,
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
            account_change: Mutex::new(()),
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
            cloud: Default::default(),
        }
    }

    async fn cloud(&self) -> TalosResult<&DesktopCloud> {
        self.cloud
            .get_or_try_init(|| async { DesktopCloud::new(&self.config.cloud) })
            .await
    }

    async fn conversation_cloud(
        &self,
        actor: &Actor,
        session: &str,
        operation: ConversationOperation<'_>,
    ) -> TalosResult<Option<&DesktopCloud>> {
        if actor.auth_disabled || !actor.role.can_use_managed_worker(actor.user_type) {
            return Ok(None);
        }
        if let Some(cloud) = self.cloud.get() {
            if let Some(personal) = cloud.personal_session(actor, session) {
                return Ok(personal.then_some(cloud));
            }
        }
        let requested_node = match operation {
            ConversationOperation::StartTurn(node) => node,
            _ => None,
        };
        let stored_node = match self.service.get_session(actor, session).await {
            // Local projections can serve reads while an ordinary Worker is offline.
            Ok(_) if matches!(operation, ConversationOperation::Read) => return Ok(None),
            // The personal Talo conversation cannot have a Project binding.
            Ok(info) if info.project_id.is_some() => return Ok(None),
            Ok(info) => Some(info.node_id),
            Err(error) if error.is_code(ErrorCode::ResourceNotFound) => None,
            Err(error) => return Err(error),
        };
        let target = stored_node.as_deref().or(requested_node);
        if let Some(target) = target {
            if target == "embedded"
                || target == self.sync_host.local_computer_id().await?
                || self
                    .sync_host
                    .sync_stores
                    .connections
                    .lock()
                    .await
                    .get(target)
                    .is_some_and(|(owner, _)| owner == &actor.user_id)
            {
                return Ok(None);
            }
        }
        let cloud = self.cloud().await?;
        // Another client can replace the account's Talo after our last status read.
        match cloud.discover_conversation(actor, session).await {
            Ok(personal) => Ok(personal.then_some(cloud)),
            // Replay the authorized local projection while its management Server is unreachable.
            Err(error)
                if matches!(operation, ConversationOperation::Timeline)
                    && stored_node.is_some()
                    && matches!(
                        error.error_code(),
                        ErrorCode::Timeout | ErrorCode::ServiceUnavailable
                    ) =>
            {
                Ok(None)
            }
            Err(error) => {
                // Ordinary Workers accept an account-scoped Coordinator. A personal Talo
                // forbids that connection, leaving its Server's original error authoritative.
                if let Some(target) = target {
                    if self.sync_host.resolve_computer(actor, target).await.is_ok() {
                        return Ok(None);
                    }
                }
                Err(error)
            }
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
                let viewer = self.media.reserve(actor, params.stream_id)?;
                self.media
                    .open(
                        &self.service,
                        self.cloud.get(),
                        actor,
                        params.ticket,
                        viewer,
                    )
                    .await?;
                value(())
            }
            ClientRequestParams::DesktopStreamSend(params) => {
                self.media.send(actor, params)?;
                value(())
            }
            ClientRequestParams::DesktopStreamClose(params) => {
                self.media.close(actor, params)?;
                value(())
            }
            ClientRequestParams::DesktopWorkerIdentity(_) => value(self.worker_identity().await?),
            ClientRequestParams::DesktopWorkerEnroll(params) => {
                let _account_change = self.account_change.lock().await;
                value(self.enroll_worker(params.node_id, Some(actor)).await?)
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
    fn prepare_request(
        self: Arc<Self>,
        actor: Actor,
        request: ClientRequest,
    ) -> futures_util::future::BoxFuture<'static, ApplicationResult<ApplicationResponse>> {
        match request.params {
            ClientRequestParams::DesktopStreamOpen(params) => {
                let viewer = self
                    .media
                    .reserve(&actor, params.stream_id)
                    .map_err(project_error);
                Box::pin(async move {
                    self.media
                        .open(
                            &self.service,
                            self.cloud.get(),
                            &actor,
                            params.ticket,
                            viewer?,
                        )
                        .await
                        .map_err(project_error)?;
                    Ok(ApplicationResponse {
                        body: serde_json::Value::Null,
                        started_turn: None,
                    })
                })
            }
            ClientRequestParams::DesktopStreamSend(params) => {
                media_response(self.media.send(&actor, params))
            }
            ClientRequestParams::DesktopStreamClose(params) => {
                media_response(self.media.close(&actor, params))
            }
            _ => Box::pin(async move { self.dispatch(&actor, request).await }),
        }
    }

    fn close_connection(&self) {
        self.media.close_all();
    }

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
        if let Some((session, attachment)) = ChatAttachment::for_request(&request.params) {
            // A locally available file can be read while its ordinary Worker is offline.
            let local_read = matches!(
                request.params,
                ClientRequestParams::FileRead(_) | ClientRequestParams::FileContent(_)
            ) || matches!(
                &attachment,
                ChatAttachment::Preview(talos_types::FilePreviewSource::File { .. })
            );
            let local_error = if local_read
                && !self
                    .cloud
                    .get()
                    .is_some_and(|cloud| cloud.personal_session(actor, session) == Some(true))
            {
                match self.local.dispatch(actor, request.clone()).await {
                    Err(error)
                        if matches!(
                            error.code,
                            ErrorCode::ResourceNotFound | ErrorCode::ServiceUnavailable
                        ) =>
                    {
                        Some(error)
                    }
                    result => return result,
                }
            } else {
                None
            };
            let Some(cloud) = self
                .conversation_cloud(actor, session, ConversationOperation::Command)
                .await
                .map_err(project_error)?
            else {
                if let Some(error) = local_error {
                    return Err(error);
                }
                return self.local.dispatch(actor, request).await;
            };
            let call = attachment
                .cloud_call(self.service.files.limits().max_upload_bytes)
                .map_err(project_error)?;
            return Ok(ApplicationResponse {
                body: cloud
                    .dispatch(actor, call, &request.params)
                    .await
                    .map_err(project_error)?,
                started_turn: None,
            });
        }
        if let Some((session, call)) = CloudCall::chat(&request.params).map_err(project_error)? {
            use ClientRequestParams::*;
            let operation = match request.params {
                TimelineRecover(_) => ConversationOperation::Timeline,
                SessionGet(_) | SessionHistory(_) | SessionActiveTurn(_) | AutomationRead(_)
                | MessageQueueList(_) => ConversationOperation::Read,
                _ => ConversationOperation::Command,
            };
            if let Some(cloud) = self
                .conversation_cloud(actor, session, operation)
                .await
                .map_err(project_error)?
            {
                return Ok(ApplicationResponse {
                    body: cloud.send(actor, call).await.map_err(project_error)?,
                    started_turn: None,
                });
            }
        }
        if let Some(call) = CloudCall::for_request(actor, &request.params, &self.sync_host)
            .await
            .map_err(project_error)?
        {
            if let Some(node) = CloudCall::personal_target(&request.params).filter(|node| {
                !self
                    .cloud
                    .get()
                    .is_some_and(|cloud| cloud.knows_personal_node(actor, node))
            }) {
                let result = self.local.dispatch(actor, request.clone()).await;
                // On cold startup, the ordinary Worker route remains authoritative unless the
                // Server confirms this is the account's personal Talo.
                if !matches!(&result, Err(error) if matches!(error.code, ErrorCode::PermissionDenied | ErrorCode::ResourceNotFound))
                    || !actor.role.can_use_managed_worker(actor.user_type)
                {
                    return result;
                }
                match self.cloud().await {
                    Ok(cloud) if cloud.discover_personal_node(actor, node).await => (),
                    _ => return result,
                }
            }
            return Ok(ApplicationResponse {
                body: self
                    .cloud()
                    .await
                    .map_err(project_error)?
                    .dispatch(actor, call, &request.params)
                    .await
                    .map_err(project_error)?,
                started_turn: None,
            });
        }
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
                let _media_suspension = self.media.suspend();
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
                // Keep explicit enrollment out until the account is cleared.
                // Shared Sync can acquire the Worker lock under its own lifecycle.
                let _account_change = self.account_change.lock().await;
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
        if let Some(cloud) = self
            .conversation_cloud(
                actor,
                &request.session_id,
                ConversationOperation::StartTurn(request.node_id.as_deref()),
            )
            .await
            .map_err(project_error)?
        {
            return Ok(ApplicationResponse {
                body: cloud
                    .send(
                        actor,
                        CloudCall::post("/v1/turns/start", &request).map_err(project_error)?,
                    )
                    .await
                    .map_err(project_error)?,
                // The Server owns the turn even if the Desktop closes.
                started_turn: None,
            });
        }
        self.local.start_turn(actor, source, request).await
    }

    async fn subscribe_timeline_events(
        &self,
        actor: &Actor,
        params: SubscribeTimelineRequest,
    ) -> ApplicationResult<talos_app_server::ApplicationSubscription> {
        if let Some(cloud) = self
            .conversation_cloud(actor, &params.session_id, ConversationOperation::Timeline)
            .await
            .map_err(project_error)?
        {
            return cloud.subscribe(actor, params).await.map_err(project_error);
        }
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
        self.media.close_all();
        self.stop_registration().await?;
        self.local.drain_background_tasks(timeout).await
    }
}

fn media_response(
    result: TalosResult<()>,
) -> futures_util::future::BoxFuture<'static, ApplicationResult<ApplicationResponse>> {
    Box::pin(async move {
        result.map_err(project_error)?;
        Ok(ApplicationResponse {
            body: serde_json::Value::Null,
            started_turn: None,
        })
    })
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
