//! Native chat uses the conversation identity returned by the personal Talo's Server.
use super::*;
use futures_util::StreamExt;
use serde::Serialize;
use talos_app_protocol::events::{SubscriptionTerminatedEvent, SubscriptionTerminatedReason};
use talos_app_server::ApplicationSubscription;
use talos_service::{
    TimelineSubscriptionCloseReason, TimelineSubscriptionClosed,
    TimelineSubscriptionMessage as Message,
};
use talos_types::{SubscribeTimelineRequest, SubscribeTimelineResponse};

impl CloudCall {
    pub(crate) fn post(path: &str, params: &impl Serialize) -> TalosResult<Self> {
        Ok(Self {
            method: Method::POST,
            path: path.into(),
            body: Some(value(params)?),
        })
    }

    pub(super) fn get_query(path: &str, query: &impl Serialize) -> TalosResult<Self> {
        let query = serde_urlencoded::to_string(query)
            .map_err(|e| TalosError::internal_with_source("Encode Server query.", e))?;
        Ok(Self {
            method: Method::GET,
            path: format!("{path}?{query}"),
            body: None,
        })
    }
}

impl DesktopCloud {
    pub(crate) fn personal_session(&self, actor: &Actor, session: &str) -> Option<bool> {
        self.cached_personal_status(actor, |status| {
            status
                .instance
                .as_ref()
                .and_then(|info| info.conversation_session_id.as_deref())
                == Some(session)
        })
    }

    pub(crate) fn chat_call(
        &self,
        actor: &Actor,
        request: &ClientRequestParams,
    ) -> TalosResult<Option<CloudCall>> {
        use ClientRequestParams::*;
        let mine = |id: &str| self.personal_session(actor, id) == Some(true);
        let call = match request {
            SessionGet(p) if mine(&p.session_id) => {
                CloudCall::get_query(&format!("/v1/sessions/{}", p.session_id), &())?
            }
            SessionPatch(p) if mine(&p.session_id) => CloudCall {
                method: Method::PATCH,
                path: format!("/v1/sessions/{}", p.session_id),
                body: Some(value(p)?),
            },
            SessionHistory(p) if mine(&p.session_id) => CloudCall::get_query(
                &format!("/v1/sessions/{}/history", p.session_id),
                &[
                    ("cursor", p.cursor.clone()),
                    ("limit", p.limit.map(|n| n.to_string())),
                ],
            )?,
            SessionActiveTurn(p) if mine(&p.session_id) => {
                CloudCall::get_query(&format!("/v1/sessions/{}/active-turn", p.session_id), &())?
            }
            TimelineRecover(p) if mine(&p.session_id) => CloudCall::get_query(
                &format!("/v1/sessions/{}/timeline/recovery", p.session_id),
                &[
                    ("after_event_id", p.after_event_id.clone()),
                    ("limit", p.replay_limit.map(|n| n.to_string())),
                ],
            )?,
            TurnRetry(p) if mine(&p.session_id) => CloudCall::post("/v1/turns/retry", p)?,
            TurnInput(p) if mine(&p.identity.session_id) => CloudCall::post("/v1/turns/input", p)?,
            TurnInterrupt(p) if mine(&p.identity.session_id) => {
                CloudCall::post("/v1/turns/interrupt", p)?
            }
            AutomationRead(p) if mine(&p.session_id) => CloudCall::post("/v1/automation/read", p)?,
            MessageQueueList(p) if mine(&p.session_id) => {
                CloudCall::post("/v1/message-queue/list", p)?
            }
            MessageQueueAdd(p) if mine(&p.session_id) => {
                CloudCall::post("/v1/message-queue/add", p)?
            }
            MessageQueueUpdate(p) if mine(&p.session_id) => {
                CloudCall::post("/v1/message-queue/update", p)?
            }
            MessageQueueDelete(p) if mine(&p.session_id) => {
                CloudCall::post("/v1/message-queue/delete", p)?
            }
            MessageQueueReorder(p) if mine(&p.session_id) => {
                CloudCall::post("/v1/message-queue/reorder", p)?
            }
            MessageQueueStart(p) if mine(&p.session_id) => {
                CloudCall::post("/v1/message-queue/start", p)?
            }
            _ => return Ok(None),
        };
        Ok(Some(call))
    }

    pub(crate) async fn subscribe(
        &self,
        actor: &Actor,
        p: SubscribeTimelineRequest,
    ) -> TalosResult<ApplicationSubscription> {
        let url = self
            .origin
            .join(&format!("/v1/sessions/{}/events", p.session_id))
            .map_err(|_| invalid_origin())?;
        let request = self
            .client
            .get(url)
            .bearer_auth(
                actor
                    .access_token
                    .as_deref()
                    .ok_or_else(TalosError::authorization_denied)?,
            )
            .query(&[
                ("after_event_id", p.since_event_id),
                ("turn_id", p.turn_id),
                ("limit", p.replay_limit.map(|v| v.to_string())),
                ("close_when_idle", p.close_when_idle.map(|v| v.to_string())),
            ])
            .send();
        let response = tokio::time::timeout(Duration::from_secs(60), request)
            .await
            .map_err(|e| TalosError::from_source(ErrorCode::Timeout, "Open Server timeline.", e))?
            .map_err(request_error)?;
        if !response.status().is_success() {
            return Err(
                match response
                    .json::<ApiResponse<Value>>()
                    .await
                    .map_err(request_error)?
                {
                    ApiResponse::Error { error } => error.into(),
                    _ => TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Invalid Server event stream.",
                    ),
                },
            );
        }
        let (tx, events) = tokio::sync::mpsc::channel(64);
        tokio::spawn(async move {
            let mut stream = sse_stream::SseStream::from_bytes_stream(response.bytes_stream());
            loop {
                let next =
                    tokio::select! { _ = tx.closed() => break, next = stream.next() => next };
                let event = match next {
                    Some(Ok(event)) => event,
                    Some(Err(error)) => {
                        talos_logging::report_error(&TalosError::internal_with_source(
                            "Read Server timeline.",
                            error,
                        ));
                        break;
                    }
                    None => break,
                };
                let Some(data) = event.data else { continue };
                match decode_message(event.event.as_deref().unwrap_or_default(), &data) {
                    Ok(message) => {
                        if tx.send(message).await.is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        talos_logging::report_error(&TalosError::internal_with_source(
                            "Decode Server timeline.",
                            error,
                        ));
                        break;
                    }
                }
            }
        });
        Ok(ApplicationSubscription {
            response: SubscribeTimelineResponse {
                subscription_id: uuid::Uuid::new_v4().to_string(),
                session_id: p.session_id,
                high_water_mark_event_id: None,
            },
            events,
        })
    }
}

fn decode_message(name: &str, data: &str) -> Result<Message, serde_json::Error> {
    Ok(match name {
        "session/activeTurnChanged" => Message::ActiveTurnChanged(serde_json::from_str(data)?),
        "messageQueue/changed" => Message::MessageQueueChanged(serde_json::from_str(data)?),
        "automation/changed" => Message::AutomationChanged(serde_json::from_str(data)?),
        "subscription/terminated" => {
            let event: SubscriptionTerminatedEvent = serde_json::from_str(data)?;
            let reason = match event.reason {
                SubscriptionTerminatedReason::Completed => {
                    TimelineSubscriptionCloseReason::Completed
                }
                SubscriptionTerminatedReason::Lagged
                | SubscriptionTerminatedReason::SlowConsumer => {
                    TimelineSubscriptionCloseReason::Lagged
                }
                _ => TimelineSubscriptionCloseReason::ServerShutdown,
            };
            Message::Closed(TimelineSubscriptionClosed {
                reason,
                message: event.message,
            })
        }
        _ => Message::Event(serde_json::from_str(data)?),
    })
}
