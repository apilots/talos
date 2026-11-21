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

pub(crate) enum ChatAttachment<'a> {
    Upload(&'a talos_app_protocol::requests::FileUploadParams),
    List(&'a talos_types::FileListRequest),
    File {
        id: &'a str,
        method: Method,
        suffix: &'static str,
    },
}

impl<'a> ChatAttachment<'a> {
    pub(crate) fn for_request(request: &'a ClientRequestParams) -> Option<(&'a str, Self)> {
        use ClientRequestParams::*;
        match request {
            FileUpload(p) => Some((p.session_id.as_deref()?, Self::Upload(p))),
            FileList(p) => Some((p.session_id.as_deref()?, Self::List(&p.request))),
            FileRead(p) | FileContent(p) | FileDelete(p) => {
                let (method, suffix) = match request {
                    FileContent(_) => (Method::GET, "/content"),
                    FileDelete(_) => (Method::DELETE, ""),
                    _ => (Method::GET, ""),
                };
                Some((
                    p.session_id.as_deref()?,
                    Self::File {
                        id: &p.file_id,
                        method,
                        suffix,
                    },
                ))
            }
            _ => None,
        }
    }

    pub(crate) fn cloud_call(self, max_upload_bytes: u64) -> TalosResult<CloudCall> {
        match self {
            Self::Upload(p) => {
                let bytes =
                    talos_app_server::decode_file_upload_data(&p.data_base64, max_upload_bytes)?;
                let metadata = serde_json::to_string(&p.request).map_err(|error| {
                    TalosError::internal_with_source("Encode file metadata.", error)
                })?;
                Ok(CloudCall {
                    method: Method::POST,
                    path: "/v1/files".into(),
                    body: Some(CloudBody::Upload {
                        metadata,
                        filename: p.request.filename.clone(),
                        bytes,
                    }),
                })
            }
            Self::List(request) => {
                let mut query = vec![
                    ("cursor", request.cursor.clone()),
                    ("limit", request.limit.map(|v| v.to_string())),
                ];
                match &request.scope {
                    Some(talos_types::FileScope::Personal) => {
                        query.push(("scope", Some("personal".into())))
                    }
                    Some(talos_types::FileScope::Project { project_id }) => {
                        query.push(("scope", Some("project".into())));
                        query.push(("projectId", Some(project_id.clone())));
                    }
                    None => (),
                }
                query.extend(
                    request
                        .tags
                        .iter()
                        .flatten()
                        .map(|tag| ("tag", Some(tag.clone()))),
                );
                CloudCall::get_query("/v1/files", &query)
            }
            Self::File { id, method, suffix } => {
                let id = uuid::Uuid::parse_str(id).map_err(|error| {
                    TalosError::from_source(ErrorCode::InvalidRequest, "Invalid file ID.", error)
                })?;
                Ok(CloudCall {
                    method,
                    path: format!("/v1/files/{id}{suffix}"),
                    body: None,
                })
            }
        }
    }
}

impl CloudCall {
    pub(crate) fn post(path: &str, params: &impl Serialize) -> TalosResult<Self> {
        Ok(Self {
            method: Method::POST,
            path: path.into(),
            body: Some(CloudBody::Json(value(params)?)),
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
        self.cached_personal_status(actor, |cached| {
            let personal = personal_conversation_id(&cached.response) == Some(session);
            (personal || cached.last_other_session.as_deref() == Some(session)).then_some(personal)
        })
        .flatten()
    }

    pub(crate) async fn discover_conversation(
        &self,
        actor: &Actor,
        session: &str,
    ) -> TalosResult<bool> {
        let status = self
            .read_personal_status(actor, CloudCall::managed_read(), Some(session))
            .await?;
        Ok(personal_conversation_id(&status) == Some(session))
    }
}

impl CloudCall {
    pub(crate) fn chat(request: &ClientRequestParams) -> TalosResult<Option<(&str, Self)>> {
        use ClientRequestParams::*;
        let (session, call) = match request {
            SessionGet(p) => (
                &p.session_id,
                Self::get_query(&format!("/v1/sessions/{}", p.session_id), &())?,
            ),
            SessionPatch(p) => (
                &p.session_id,
                Self {
                    method: Method::PATCH,
                    path: format!("/v1/sessions/{}", p.session_id),
                    body: Some(CloudBody::Json(value(p)?)),
                },
            ),
            SessionHistory(p) => (
                &p.session_id,
                Self::get_query(
                    &format!("/v1/sessions/{}/history", p.session_id),
                    &[
                        ("cursor", p.cursor.clone()),
                        ("limit", p.limit.map(|n| n.to_string())),
                    ],
                )?,
            ),
            SessionActiveTurn(p) => (
                &p.session_id,
                Self::get_query(&format!("/v1/sessions/{}/active-turn", p.session_id), &())?,
            ),
            TimelineRecover(p) => (
                &p.session_id,
                Self::get_query(
                    &format!("/v1/sessions/{}/timeline/recovery", p.session_id),
                    &[
                        ("after_event_id", p.after_event_id.clone()),
                        ("limit", p.replay_limit.map(|n| n.to_string())),
                    ],
                )?,
            ),
            SessionCompact(p) => (
                &p.session_id,
                Self::post(&format!("/v1/sessions/{}/compact", p.session_id), p)?,
            ),
            SessionClear(p) => (
                &p.session_id,
                Self::post(&format!("/v1/sessions/{}/clear", p.session_id), p)?,
            ),
            TurnRetry(p) => (&p.session_id, Self::post("/v1/turns/retry", p)?),
            TurnInput(p) => (&p.identity.session_id, Self::post("/v1/turns/input", p)?),
            TurnInterrupt(p) => (
                &p.identity.session_id,
                Self::post("/v1/turns/interrupt", p)?,
            ),
            AutomationExecute(p) => (&p.session_id, Self::post("/v1/automation/execute", p)?),
            AutomationRead(p) => (&p.session_id, Self::post("/v1/automation/read", p)?),
            MessageQueueList(p) => (&p.session_id, Self::post("/v1/message-queue/list", p)?),
            MessageQueueAdd(p) => (&p.session_id, Self::post("/v1/message-queue/add", p)?),
            MessageQueueUpdate(p) => (&p.session_id, Self::post("/v1/message-queue/update", p)?),
            MessageQueueDelete(p) => (&p.session_id, Self::post("/v1/message-queue/delete", p)?),
            MessageQueueReorder(p) => (&p.session_id, Self::post("/v1/message-queue/reorder", p)?),
            MessageQueueStart(p) => (&p.session_id, Self::post("/v1/message-queue/start", p)?),
            _ => return Ok(None),
        };
        Ok(Some((session, call)))
    }
}

impl DesktopCloud {
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
                (
                    "close_when_idle",
                    Some(p.close_when_idle.unwrap_or(false).to_string()),
                ),
                (
                    "close_on_terminal",
                    Some(p.close_on_terminal.unwrap_or(false).to_string()),
                ),
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
