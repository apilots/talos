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

use talos_app_protocol::events::canonical_method;
use talos_auth::RequestIdentity;
use talos_session_store::{ActiveTurnRecord, SessionEventId};

use super::access::shared_actor;
use super::errors::ApiError;
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
        .app
        .get_session(&shared_actor(&actor.0), &session_id)
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

    let mut live_rx = state.events.subscribe_live_events().map_err(|err| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "event_subscribe_failed",
            err.to_string(),
        )
    })?;
    let replay = state
        .events
        .replay_after(&session_id, after_event_id.as_ref(), limit)
        .await
        .map_err(map_event_replay_error)?;
    let has_live_turn = state
        .events
        .has_live_turn(&session_id)
        .await
        .map_err(|err| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "active_turn_failed",
                err.to_string(),
            )
        })?;

    let (tx, rx) = mpsc::channel(256);
    tokio::spawn(async move {
        let mut last_sent_seq = replay.last_replay_seq;
        for record in replay.events {
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
        let event_name = canonical_method(&record.event_type);
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

fn map_event_replay_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::ResourceExpired(message) => {
            ApiError::new(StatusCode::GONE, "event_cursor_expired", message)
        }
        talos_error::TalosError::ValidationError(message) => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_cursor", message)
        }
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "event_replay_failed",
            other.to_string(),
        ),
    }
}

pub(super) async fn get_active_turn_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _session = state
        .app
        .get_session(&shared_actor(&actor.0), &session_id)
        .await
        .map_err(|err| {
            ApiError::new(StatusCode::NOT_FOUND, "session_not_found", err.to_string())
        })?;

    let active_turn = state
        .events
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

    #[test]
    fn parse_cursor_prefers_query_over_last_event_id() {
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", "ev_00000000000000000001".parse().unwrap());

        let cursor = parse_cursor(Some("ev_00000000000000000002".to_string()), &headers)
            .unwrap()
            .unwrap();

        assert_eq!(cursor.as_str(), "ev_00000000000000000002");
    }
}
