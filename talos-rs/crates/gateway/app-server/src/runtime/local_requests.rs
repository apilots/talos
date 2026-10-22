//! Retained-data/bootstrap queries and bounded deletion share the original dispatch/auth
//! contract independently of Worker commands. Deletions have a separate capacity
//! so their remote verification cannot exhaust the local query lane.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, OnceLock,
};

use talos_app_protocol::{
    jsonrpc::{parse_message_line, JsonRpcMessage},
    requests::{ClientMethod, ClientRequest, ClientRequestParams},
};
use talos_error::{ApiError, ErrorCode};
use tokio::{sync::Semaphore, task::JoinSet};

use super::{
    resolve_request_actor, send_application_error, send_error, send_result,
    ActorCredentialValidator, AppServerApplication, OutgoingMessage,
};

pub(super) struct LocalRequests {
    initialized_actor: OnceLock<talos_service::Actor>,
    initialized_notification_seen: AtomicBool,
    subscriptions: Arc<super::subscriptions::Subscriptions>,
    app: Arc<dyn AppServerApplication>,
    validator: Arc<dyn ActorCredentialValidator>,
    auth_enabled: bool,
    query_capacity: Arc<Semaphore>,
    deletion_capacity: Arc<Semaphore>,
}

impl LocalRequests {
    pub(super) fn new(
        app: Arc<dyn AppServerApplication>,
        validator: Arc<dyn ActorCredentialValidator>,
        auth_enabled: bool,
        subscriptions: Arc<super::subscriptions::Subscriptions>,
    ) -> Self {
        Self {
            initialized_actor: OnceLock::new(),
            initialized_notification_seen: AtomicBool::new(false),
            subscriptions,
            app,
            validator,
            auth_enabled,
            query_capacity: Arc::new(Semaphore::new(16)),
            deletion_capacity: Arc::new(Semaphore::new(4)),
        }
    }

    pub(super) fn initialized(&self, actor: &talos_service::Actor) {
        self.initialized_actor.get_or_init(|| actor.clone());
    }

    pub(super) async fn dispatch(
        self: &Arc<Self>,
        line: &[u8],
        outgoing: &tokio::sync::mpsc::Sender<OutgoingMessage>,
        tasks: &mut JoinSet<()>,
    ) -> bool {
        let Ok(text) = std::str::from_utf8(line) else {
            return false;
        };
        let Ok(message) = parse_message_line(text) else {
            return false;
        };
        if let JsonRpcMessage::Notification(notification) = &message {
            if notification.method == "initialized" {
                self.initialized_notification_seen
                    .store(true, Ordering::Release);
            }
            return false;
        }
        if self.initialized_actor.get().is_none() {
            return false;
        }
        let JsonRpcMessage::Request(request) = message else {
            return false;
        };
        let Ok(method) = request.method.parse::<ClientMethod>() else {
            return false;
        };
        if !matches!(
            method,
            ClientMethod::EventSubscribe
                | ClientMethod::SyncStatus
                | ClientMethod::EventUnsubscribe
                | ClientMethod::DesktopWorkerIdentity
                | ClientMethod::AuthSessionRead
                | ClientMethod::SessionList
                | ClientMethod::SessionGet
                | ClientMethod::SessionHistory
                | ClientMethod::SessionActiveTurn
                | ClientMethod::ProjectList
                | ClientMethod::ProjectRead
                | ClientMethod::SessionDelete
                | ClientMethod::ProjectDelete
        ) {
            return false;
        }
        if matches!(
            method,
            ClientMethod::EventSubscribe | ClientMethod::EventUnsubscribe
        ) && !self.initialized_notification_seen.load(Ordering::Acquire)
        {
            return false;
        }
        let decoded = match ClientRequest::decode(&request.method, request.params) {
            Ok(decoded) => decoded,
            Err(error) => {
                let _ = send_error(outgoing.clone(), Some(request.id), error).await;
                return true;
            }
        };
        let capacity = if matches!(
            method,
            ClientMethod::SessionDelete | ClientMethod::ProjectDelete
        ) {
            &self.deletion_capacity
        } else {
            &self.query_capacity
        };
        let Ok(permit) = capacity.clone().try_acquire_owned() else {
            let _ = send_application_error(
                outgoing.clone(),
                request.id,
                ApiError::new(ErrorCode::Conflict),
            )
            .await;
            return true;
        };
        let requests = self.clone();
        let outgoing = outgoing.clone();
        tasks.spawn(async move {
            let _permit = permit;
            let actor = match resolve_request_actor(
                requests.initialized_actor.get(),
                requests.validator.as_ref(),
                requests.auth_enabled,
                decoded.method,
                request.actor_credentials,
            )
            .await
            {
                Ok(actor) => actor,
                Err(error) => {
                    let _ = send_application_error(outgoing, request.id, error).await;
                    return;
                }
            };
            let decoded = match decoded.params {
                ClientRequestParams::EventSubscribe(params) => {
                    let _ = requests
                        .subscriptions
                        .subscribe(requests.app.as_ref(), &actor, params, outgoing, request.id)
                        .await;
                    return;
                }
                ClientRequestParams::EventUnsubscribe(params) => {
                    let _ = requests
                        .subscriptions
                        .unsubscribe(params, outgoing, request.id)
                        .await;
                    return;
                }
                params => ClientRequest {
                    method: decoded.method,
                    params,
                },
            };
            match requests.app.dispatch(&actor, decoded).await {
                Ok(response) => {
                    let _ = send_result(outgoing, request.id, Ok(response.body)).await;
                }
                Err(error) => {
                    let _ = send_application_error(outgoing, request.id, error).await;
                }
            }
        });
        true
    }
}
