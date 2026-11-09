//! Connection-owned scheduling for initialized JSON-RPC requests.
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
};

use futures_util::future::BoxFuture;
use talos_app_protocol::requests::{ClientMethod, ClientRequest, ClientRequestParams};
use talos_error::{ApiError, ErrorCode, TalosError};
use talos_logging::report_error;
use talos_service::Actor;
use talos_types::talos::SessionSource;
use tokio::{
    sync::{mpsc, OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
};

use tokio_util::sync::CancellationToken;

use super::{
    send_application_error, send_result, subscriptions::Subscriptions, AppServerApplication,
    OutgoingMessage, RequestId,
};
use crate::application::{ApplicationResponse, ApplicationResult};

pub(super) type Dispatch = BoxFuture<'static, ()>;
#[derive(Default)]
struct OwnedTurns {
    closing: bool,
    sessions: HashMap<String, Vec<OwnedTurn>>,
}

struct OwnedTurn {
    actor: Actor,
    turn_id: String,
}

pub(super) struct RequestScheduler {
    local_read: Arc<Semaphore>,
    remote_read: Arc<Semaphore>,
    deletion: Arc<Semaphore>,
    disconnect: CancellationToken,
    general: Arc<Semaphore>,
    control: Arc<Semaphore>,
    tasks: JoinSet<()>,
    owned_turns: Arc<Mutex<OwnedTurns>>,
}

#[derive(Clone, Copy)]
pub(super) enum RequestClass {
    General,
    Control,
    LocalRead,
    RemoteRead,
    Deletion,
}

impl RequestClass {
    pub(super) fn for_method(method: ClientMethod) -> Self {
        match method {
            ClientMethod::TurnInterrupt
            | ClientMethod::TurnInput
            | ClientMethod::DesktopStreamSend
            | ClientMethod::DesktopStreamClose
            | ClientMethod::AuthLogout
            | ClientMethod::AuthSessionClear => Self::Control,
            ClientMethod::ComputerList => Self::RemoteRead,
            ClientMethod::SessionDelete | ClientMethod::ProjectDelete => Self::Deletion,
            ClientMethod::EventSubscribe
            | ClientMethod::EventUnsubscribe
            | ClientMethod::SyncStatus
            | ClientMethod::DesktopWorkerIdentity
            | ClientMethod::AuthSessionRead
            | ClientMethod::SessionList
            | ClientMethod::SessionGet
            | ClientMethod::SessionHistory
            | ClientMethod::TrajectoryCapabilities
            | ClientMethod::TrajectoryReadTurn
            | ClientMethod::TrajectoryReadRequest
            | ClientMethod::TrajectoryReadTool
            | ClientMethod::TrajectoryReadPayload
            | ClientMethod::SessionActiveTurn
            | ClientMethod::ProjectList
            | ClientMethod::ProjectRead => Self::LocalRead,
            _ => Self::General,
        }
    }

    fn cancel_on_disconnect(self) -> bool {
        // A client can close stdin and still read replies to accepted local
        // queries. Remote inventory and deletion may be waiting on a Worker.
        matches!(self, Self::RemoteRead | Self::Deletion)
    }
}

pub(super) struct Admission {
    permit: OwnedSemaphorePermit,
    class: RequestClass,
}

impl RequestScheduler {
    pub(super) fn new() -> Self {
        Self {
            general: Arc::new(Semaphore::new(16)),
            control: Arc::new(Semaphore::new(4)),
            local_read: Arc::new(Semaphore::new(16)),
            remote_read: Arc::new(Semaphore::new(4)),
            deletion: Arc::new(Semaphore::new(4)),
            disconnect: CancellationToken::new(),
            tasks: JoinSet::new(),
            owned_turns: Arc::default(),
        }
    }

    pub(super) fn reserve(&self, method: ClientMethod) -> Result<Admission, ApiError> {
        let class = RequestClass::for_method(method);
        let capacity = match class {
            RequestClass::General => &self.general,
            RequestClass::Control => &self.control,
            RequestClass::LocalRead => &self.local_read,
            RequestClass::RemoteRead => &self.remote_read,
            RequestClass::Deletion => &self.deletion,
        };
        let permit = capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| ApiError::new(ErrorCode::Conflict))?;
        Ok(Admission { permit, class })
    }

    pub(super) fn prepare(
        &self,
        app: Arc<dyn AppServerApplication>,
        source: SessionSource,
        request: ClientRequest,
        subscriptions: Arc<Subscriptions>,
        id: RequestId,
        outgoing: mpsc::Sender<OutgoingMessage>,
    ) -> impl FnOnce(Actor) -> Dispatch + Send + 'static {
        let owned = self.owned_turns.clone();
        move |actor| {
            let params = match request.params {
                ClientRequestParams::EventSubscribe(params) => {
                    return Box::pin(async move {
                        let _ = subscriptions
                            .subscribe(app.as_ref(), &actor, params, outgoing, id)
                            .await;
                    });
                }
                ClientRequestParams::EventUnsubscribe(params) => {
                    return Box::pin(async move {
                        let _ = subscriptions.unsubscribe(params, outgoing, id).await;
                    });
                }
                ClientRequestParams::TurnStart(params) => params,
                _ => {
                    let response = app.prepare_request(actor, request);
                    return Box::pin(async move { reply(outgoing, id, response.await).await });
                }
            };
            Box::pin(async move {
                let response = async {
                    let previous: Vec<_> = owned
                        .lock()
                        .expect("owned Turns lock")
                        .sessions
                        .get(&params.session_id)
                        .into_iter()
                        .flatten()
                        .map(|turn| turn.turn_id.clone())
                        .collect();
                    let response = app.start_turn(&actor, source, params).await?;
                    if let Some(identity) = &response.started_turn {
                        let closing = {
                            let mut owned = owned.lock().expect("owned Turns lock");
                            if !owned.closing {
                                let turns = owned
                                    .sessions
                                    .entry(identity.session_id.clone())
                                    .or_default();
                                // An overlapping admission can return out of order.
                                // Keep the identities admitted while this request waited.
                                if !previous.contains(&identity.turn_id) {
                                    turns.retain(|turn| !previous.contains(&turn.turn_id));
                                }
                                if !turns.iter().any(|turn| turn.turn_id == identity.turn_id) {
                                    turns.push(OwnedTurn {
                                        actor: actor.clone(),
                                        turn_id: identity.turn_id.clone(),
                                    });
                                }
                            }
                            owned.closing
                        };
                        if closing {
                            let _ = app
                                .cancel_active_turn(&actor, &identity.session_id, &identity.turn_id)
                                .await;
                        }
                    }
                    Ok(response)
                }
                .await;
                reply(outgoing, id, response).await;
            })
        }
    }

    pub(super) fn spawn(
        &mut self,
        request: impl Future<Output = ()> + Send + 'static,
        admission: Admission,
        frame_permit: Option<OwnedSemaphorePermit>,
    ) {
        while let Some(result) = self.tasks.try_join_next() {
            report_task(result);
        }
        let disconnect = self.disconnect.clone();
        self.tasks.spawn(async move {
            let _permit = admission.permit;
            let _frame_permit = frame_permit;
            if admission.class.cancel_on_disconnect() {
                // A ready reply can still be written after stdin is half-closed.
                tokio::select! {
                    biased;
                    () = request => {}
                    () = disconnect.cancelled() => {}
                }
            } else {
                request.await;
            }
        });
    }

    pub(super) fn has_tasks(&self) -> bool {
        !self.tasks.is_empty()
    }

    pub(super) async fn reap(&mut self) {
        if let Some(result) = self.tasks.join_next().await {
            report_task(result);
        }
    }

    pub(super) async fn drain(&mut self, app: &dyn AppServerApplication) {
        self.disconnect.cancel();
        let owned = {
            let mut owned = self.owned_turns.lock().expect("owned Turns lock");
            owned.closing = true;
            std::mem::take(&mut owned.sessions)
        };
        for (session_id, turns) in owned {
            for turn in turns {
                let _ = app
                    .cancel_active_turn(&turn.actor, &session_id, &turn.turn_id)
                    .await;
            }
        }
        while self.has_tasks() {
            self.reap().await;
        }
    }
}

async fn reply(
    outgoing: mpsc::Sender<OutgoingMessage>,
    id: RequestId,
    response: ApplicationResult<ApplicationResponse>,
) {
    match response {
        Ok(response) => {
            let _ = send_result(outgoing, id, Ok(response.body)).await;
        }
        Err(error) => {
            let _ = send_application_error(outgoing, id, error).await;
        }
    }
}

fn report_task(result: Result<(), tokio::task::JoinError>) {
    if let Err(error) = result {
        report_error(&TalosError::internal_with_source(
            "App-server request task failed.",
            error,
        ));
    }
}
