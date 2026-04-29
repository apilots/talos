use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tracing::error;

use talos_auth::RequestIdentity;
use talos_session_store::{ActiveTurnRecord, ActiveTurnStatus, SessionEventId};

use super::errors::ApiError;
use super::misc::talos_event_name;
use super::state::HttpApiState;

#[derive(Debug, Deserialize)]
pub(super) struct SessionEventsQuery {
    after_event_id: Option<String>,
    limit: Option<usize>,
}

pub(super) async fn subscribe_session_events_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<SessionEventsQuery>,
) -> Result<Response, ApiError> {
    let _session = state
        .access_service
        .get_session(&actor.0, &session_id)
        .await
        .map_err(|err| {
            ApiError::new(StatusCode::NOT_FOUND, "session_not_found", err.to_string())
        })?;

    let after_event_id = parse_cursor(query.after_event_id, &headers)?;
    let limit = query
        .limit
        .unwrap_or(
            state
                .base_config
                .agent
                .kernel
                .session_events
                .replay_batch_size,
        )
        .max(1);

    let mut live_rx = state.event_service.subscribe_live_events();
    let replay = state
        .event_service
        .list_events_after(&session_id, after_event_id.as_ref(), limit)
        .await
        .map_err(|err| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "event_replay_failed",
                err.to_string(),
            )
        })?;
    let oldest_event_id = if after_event_id.is_some() {
        state
            .event_service
            .oldest_event_id(&session_id)
            .await
            .map_err(|err| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "event_replay_failed",
                    err.to_string(),
                )
            })?
    } else {
        None
    };
    reject_expired_cursor(
        after_event_id.as_ref(),
        replay.first(),
        oldest_event_id.as_ref(),
    )?;
    let has_live_turn = state
        .event_service
        .get_active_turn(&session_id)
        .await
        .map_err(|err| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "active_turn_failed",
                err.to_string(),
            )
        })?
        .is_some_and(|record| {
            matches!(
                record.status,
                ActiveTurnStatus::Running
                    | ActiveTurnStatus::WaitingUserAction
                    | ActiveTurnStatus::Cancelling
            )
        });

    let last_replay_seq = replay
        .last()
        .map(|event| event.event_seq)
        .unwrap_or_else(|| {
            after_event_id
                .as_ref()
                .and_then(|event_id| event_id.event_seq().ok())
                .unwrap_or_default()
        });

    let (tx, rx) = mpsc::channel(256);
    tokio::spawn(async move {
        let mut last_sent_seq = last_replay_seq;
        for record in replay {
            last_sent_seq = record.event_seq;
            if tx.send(record).await.is_err() {
                return;
            }
        }

        if !has_live_turn {
            return;
        }

        loop {
            match live_rx.recv().await {
                Ok(record) => {
                    if record.session_id != session_id || record.event_seq <= last_sent_seq {
                        continue;
                    }
                    last_sent_seq = record.event_seq;
                    let is_terminal = is_terminal_event(&record.event_type);
                    if tx.send(record).await.is_err() {
                        return;
                    }
                    if is_terminal {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });

    let stream = ReceiverStream::new(rx).map(|record| {
        let event_name = talos_event_name(&record.event_type);
        let data = match serde_json::to_string(&record) {
            Ok(data) => data,
            Err(err) => {
                error!(error = %err, "failed to serialize session event record");
                json!({
                    "event_type": "message_error",
                    "session_id": record.session_id,
                    "message_id": record.message_id,
                    "sequence": record.sequence,
                    "payload": { "type": "message_error", "request_id": "unknown", "error": "failed to serialize session event" }
                })
                .to_string()
            }
        };
        Ok::<Event, Infallible>(
            Event::default()
                .id(record.event_id.to_string())
                .event(event_name)
                .data(data),
        )
    });

    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}

fn is_terminal_event(event_type: &talos_types::talos::TalosEventType) -> bool {
    matches!(
        event_type,
        talos_types::talos::TalosEventType::MessageCompleted
            | talos_types::talos::TalosEventType::MessageError
    )
}

fn parse_cursor(
    query_after_event_id: Option<String>,
    headers: &HeaderMap,
) -> Result<Option<SessionEventId>, ApiError> {
    let cursor = query_after_event_id.or_else(|| {
        headers
            .get("last-event-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    });
    match cursor {
        Some(value) => {
            let event_id = SessionEventId::from(value);
            event_id.event_seq().map_err(|err| {
                ApiError::new(StatusCode::BAD_REQUEST, "invalid_cursor", err.to_string())
            })?;
            Ok(Some(event_id))
        }
        None => Ok(None),
    }
}

fn reject_expired_cursor(
    after_event_id: Option<&SessionEventId>,
    first_replay_event: Option<&talos_session_store::SessionEventRecord>,
    oldest_event_id: Option<&SessionEventId>,
) -> Result<(), ApiError> {
    let Some(after_event_id) = after_event_id else {
        return Ok(());
    };
    let after_seq = after_event_id
        .event_seq()
        .map_err(|err| ApiError::new(StatusCode::BAD_REQUEST, "invalid_cursor", err.to_string()))?;
    if let Some(oldest_event_id) = oldest_event_id {
        let oldest_seq = oldest_event_id.event_seq().map_err(|err| {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_cursor", err.to_string())
        })?;
        if oldest_seq > after_seq + 1 {
            return Err(expired_cursor_error());
        }
    }
    if first_replay_event.is_some_and(|event| event.event_seq > after_seq + 1) {
        return Err(expired_cursor_error());
    }
    Ok(())
}

fn expired_cursor_error() -> ApiError {
    ApiError::new(
        StatusCode::GONE,
        "event_cursor_expired",
        "event cursor is older than retained session events",
    )
}

pub(super) async fn get_active_turn_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _session = state
        .access_service
        .get_session(&actor.0, &session_id)
        .await
        .map_err(|err| {
            ApiError::new(StatusCode::NOT_FOUND, "session_not_found", err.to_string())
        })?;

    let active_turn = state
        .event_service
        .get_active_turn(&session_id)
        .await
        .map_err(|err| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "active_turn_failed",
                err.to_string(),
            )
        })?;

    Ok(Json(render_active_turn(&session_id, active_turn)))
}

fn render_active_turn(
    session_id: &str,
    active_turn: Option<ActiveTurnRecord>,
) -> serde_json::Value {
    match active_turn {
        Some(record) => json!({
            "session_id": record.session_id,
            "turn_id": record.turn_id,
            "message_id": record.message_id,
            "status": record.status,
            "last_event_id": record.last_event_id.map(|value| value.to_string()),
            "pending_user_action": record.pending_user_action,
            "updated_at": record.updated_at,
        }),
        None => json!({
            "session_id": session_id,
            "turn_id": null,
            "message_id": null,
            "status": "completed",
            "last_event_id": null,
            "pending_user_action": null,
            "updated_at": chrono::Utc::now().timestamp_millis(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use talos_types::talos::{
        TalosEventPayload, TalosEventType, TalosMessageStartedEvent, TalosUsage,
    };

    #[test]
    fn parse_cursor_prefers_query_over_last_event_id() {
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", "ev_00000000000000000001".parse().unwrap());

        let cursor = parse_cursor(Some("ev_00000000000000000002".to_string()), &headers)
            .unwrap()
            .unwrap();

        assert_eq!(cursor.as_str(), "ev_00000000000000000002");
    }

    #[test]
    fn reject_expired_cursor_returns_gone_on_replay_gap() {
        let first_replay_event = talos_session_store::SessionEventRecord {
            event_seq: 5,
            event_id: SessionEventId::from_seq(5),
            session_id: "s1".to_string(),
            turn_id: Some("turn-1".to_string()),
            message_id: "msg-1".to_string(),
            sequence: 0,
            event_type: TalosEventType::MessageStarted,
            payload: TalosEventPayload::MessageStarted(TalosMessageStartedEvent {
                request_id: "req-1".to_string(),
                model: "qwen-plus".to_string(),
                usage: TalosUsage::default(),
                iteration: 0,
                step_id: "start".to_string(),
            }),
            created_at: 1,
        };

        let err = reject_expired_cursor(
            Some(&SessionEventId::from_seq(1)),
            Some(&first_replay_event),
            Some(&SessionEventId::from_seq(5)),
        )
        .expect_err("cursor gap should be rejected");

        assert_eq!(err.into_response().status(), StatusCode::GONE);
    }

    #[test]
    fn reject_expired_cursor_returns_gone_on_empty_replay_gap() {
        let err = reject_expired_cursor(
            Some(&SessionEventId::from_seq(1)),
            None,
            Some(&SessionEventId::from_seq(5)),
        )
        .expect_err("cursor older than oldest retained event should be rejected");

        assert_eq!(err.into_response().status(), StatusCode::GONE);
    }
}
