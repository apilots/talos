mod codec;
mod dispatch;
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

/// Serialized result of one typed application dispatch for JSON-RPC.
/// `started_turn` carries the canonical identity for connection-owned lifecycle
/// control; transports must not recover that identity by inspecting JSON fields.
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
        self.dispatch_request(actor, request).await
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
