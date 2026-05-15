use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::json;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tracing::error;

use talos_app_protocol::events::canonical_method;
use talos_app_protocol::requests::{
    TimelineActiveTurnSnapshot, TimelineActiveTurnStatus, TimelineCursorStatus,
    TimelinePendingDynamicToolResult, TimelinePendingToolApproval, TimelinePendingUserAction,
    TimelineRecoverySnapshot, TimelineReplay,
};
use talos_application_service::TimelineCursorStatus as ApplicationTimelineCursorStatus;
use talos_application_service::TimelineSubscribeParams;
use talos_auth::RequestIdentity;
use talos_session_store::{ActiveTurnRecord, ActiveTurnStatus, PendingUserAction, TimelineEventId};

use super::access::shared_actor;
use super::errors::ApiError;
use super::state::HttpApiState;

#[derive(Debug, Deserialize)]
pub(super) struct TimelineEventsQuery {
    after_event_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TimelineRecoveryQuery {
    after_event_id: Option<String>,
    limit: Option<usize>,
}

pub(super) async fn subscribe_timeline_events_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<TimelineEventsQuery>,
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
                .timeline_events
                .replay_batch_size,
        )
        .max(1);

    let subscription = state
        .events
        .subscribe(TimelineSubscribeParams {
            session_id: session_id.clone(),
            after_event_id,
            replay_limit: limit,
            close_when_idle: true,
            close_on_terminal: true,
        })
        .await
        .map_err(map_event_replay_error)?;

    let stream = ReceiverStream::new(subscription.events).map(|record| {
        let event_name = canonical_method(&record.event.event_type);
        let data = match serde_json::to_string(&record) {
            Ok(data) => data,
            Err(err) => {
                error!(error = %err, "failed to serialize timeline event record");
                let turn_id = record.turn_id.clone();
                json!({
                    "event_id": record.event_id,
                    "event_seq": record.event_seq,
                    "created_at_ms": record.created_at_ms,
                    "session_id": record.session_id,
                    "message_id": record.message_id,
                    "turn_id": turn_id,
                    "event": {
                        "event_type": "turn_error",
                        "session_id": record.session_id,
                        "message_id": record.message_id,
                        "sequence": 0,
                        "turn_id": turn_id,
                        "payload": { "type": "turn_error", "request_id": "unknown", "turn_id": turn_id, "error": "failed to serialize timeline event", "failed_at_ms": 0 }
                    }
                })
                .to_string()
            }
        };
        Ok::<Event, Infallible>(
            Event::default()
                .id(record.event_id)
                .event(event_name)
                .data(data),
        )
    });

    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}

pub(super) async fn recover_timeline_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<TimelineRecoveryQuery>,
) -> Result<Json<TimelineRecoverySnapshot>, ApiError> {
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
                .timeline_events
                .replay_batch_size,
        )
        .max(1);
    let snapshot = state
        .events
        .recover_session(&session_id, after_event_id.as_ref(), limit)
        .await
        .map_err(map_event_replay_error)?;

    Ok(Json(TimelineRecoverySnapshot {
        replay: TimelineReplay {
            events: snapshot.replay.events,
            high_water_mark_event_id: snapshot
                .replay
                .high_water_mark_event_id
                .map(|value| value.to_string()),
            last_replay_seq: snapshot.replay.last_replay_seq,
            cursor_status: map_timeline_cursor_status(snapshot.replay.cursor_status),
        },
        active_turn: map_active_turn_snapshot(&session_id, snapshot.active_turn),
    }))
}

fn parse_cursor(
    query_after_event_id: Option<String>,
    headers: &HeaderMap,
) -> Result<Option<TimelineEventId>, ApiError> {
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
            let event_id = TimelineEventId::from(value);
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
    serde_json::to_value(map_active_turn_snapshot(session_id, active_turn))
        .unwrap_or_else(|_| json!({ "session_id": session_id, "status": "failed" }))
}

fn map_active_turn_snapshot(
    session_id: &str,
    active_turn: Option<ActiveTurnRecord>,
) -> TimelineActiveTurnSnapshot {
    match active_turn {
        Some(record) => TimelineActiveTurnSnapshot {
            session_id: record.session_id,
            turn_id: Some(record.turn_id),
            message_id: record.message_id,
            status: map_active_turn_status(record.status),
            last_event_id: record.last_event_id.map(|value| value.to_string()),
            pending_user_action: record.pending_user_action.map(map_pending_user_action),
            updated_at: record.updated_at,
        },
        None => TimelineActiveTurnSnapshot {
            session_id: session_id.to_string(),
            turn_id: None,
            message_id: None,
            status: TimelineActiveTurnStatus::Completed,
            last_event_id: None,
            pending_user_action: None,
            updated_at: chrono::Utc::now().timestamp_millis(),
        },
    }
}

fn map_active_turn_status(status: ActiveTurnStatus) -> TimelineActiveTurnStatus {
    match status {
        ActiveTurnStatus::Running => TimelineActiveTurnStatus::Running,
        ActiveTurnStatus::WaitingUserAction => TimelineActiveTurnStatus::WaitingUserAction,
        ActiveTurnStatus::Cancelling => TimelineActiveTurnStatus::Cancelling,
        ActiveTurnStatus::Completed => TimelineActiveTurnStatus::Completed,
        ActiveTurnStatus::Failed => TimelineActiveTurnStatus::Failed,
    }
}

fn map_pending_user_action(action: PendingUserAction) -> TimelinePendingUserAction {
    match action {
        PendingUserAction::ToolApproval(pending) => {
            TimelinePendingUserAction::ToolApproval(TimelinePendingToolApproval {
                item_id: pending.item_id,
                call_id: pending.call_id,
                tool_name: pending.tool_name,
                prompt: pending.prompt,
                arguments: pending.arguments,
            })
        }
        PendingUserAction::DynamicToolResult(pending) => {
            TimelinePendingUserAction::DynamicToolResult(TimelinePendingDynamicToolResult {
                item_id: pending.item_id,
                call_id: pending.call_id,
                tool_name: pending.tool_name,
            })
        }
    }
}

fn map_timeline_cursor_status(status: ApplicationTimelineCursorStatus) -> TimelineCursorStatus {
    match status {
        ApplicationTimelineCursorStatus::Current => TimelineCursorStatus::Current,
        ApplicationTimelineCursorStatus::FromBeginning => TimelineCursorStatus::FromBeginning,
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
