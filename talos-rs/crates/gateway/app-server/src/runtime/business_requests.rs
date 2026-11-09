//! Bounded concurrent execution and connection-owned Turn cleanup.
use std::{
    collections::HashMap,
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

use super::{
    send_application_error, send_result, AppServerApplication, OutgoingMessage, RequestId,
};
use crate::application::{ApplicationResponse, ApplicationResult};

type Response = BoxFuture<'static, ApplicationResult<ApplicationResponse>>;
#[derive(Default)]
struct OwnedTurns {
    closing: bool,
    sessions: HashMap<String, Vec<OwnedTurn>>,
}

struct OwnedTurn {
    actor: Actor,
    turn_id: String,
}

pub(super) struct BusinessRequests {
    normal: Arc<Semaphore>,
    control: Arc<Semaphore>,
    tasks: JoinSet<()>,
    owned_turns: Arc<Mutex<OwnedTurns>>,
}

pub(super) fn is_control(method: ClientMethod) -> bool {
    matches!(
        method,
        ClientMethod::TurnInterrupt
            | ClientMethod::TurnInput
            | ClientMethod::DesktopStreamSend
            | ClientMethod::DesktopStreamClose
            | ClientMethod::AuthLogout
            | ClientMethod::AuthSessionClear
    )
}

impl BusinessRequests {
    pub(super) fn new() -> Self {
        Self {
            normal: Arc::new(Semaphore::new(16)),
            control: Arc::new(Semaphore::new(4)),
            tasks: JoinSet::new(),
            owned_turns: Arc::default(),
        }
    }

    pub(super) fn reserve(&self, method: ClientMethod) -> Result<OwnedSemaphorePermit, ApiError> {
        let capacity = if is_control(method) {
            &self.control
        } else {
            &self.normal
        };
        capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| ApiError::new(ErrorCode::Conflict))
    }

    pub(super) fn preparer(
        &self,
        app: Arc<dyn AppServerApplication>,
        source: SessionSource,
        request: ClientRequest,
    ) -> impl FnOnce(Actor) -> Response + Send + 'static {
        let owned = self.owned_turns.clone();
        move |actor| {
            let ClientRequestParams::TurnStart(params) = request.params else {
                return app.prepare_request(actor, request);
            };
            Box::pin(async move {
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
            })
        }
    }

    pub(super) fn spawn(
        &mut self,
        request: Response,
        id: RequestId,
        outgoing: mpsc::Sender<OutgoingMessage>,
        permit: OwnedSemaphorePermit,
        frame_permit: Option<OwnedSemaphorePermit>,
    ) {
        while let Some(result) = self.tasks.try_join_next() {
            report_task(result);
        }
        self.tasks.spawn(async move {
            let _permit = permit;
            let _frame_permit = frame_permit;
            match request.await {
                Ok(response) => {
                    let _ = send_result(outgoing, id, Ok(response.body)).await;
                }
                Err(error) => {
                    let _ = send_application_error(outgoing, id, error).await;
                }
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

fn report_task(result: Result<(), tokio::task::JoinError>) {
    if let Err(error) = result {
        report_error(&TalosError::internal_with_source(
            "App-server request task failed.",
            error,
        ));
    }
}
