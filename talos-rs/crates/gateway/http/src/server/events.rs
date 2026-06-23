use std::io;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use talos_app_protocol::events::canonical_method;
use talos_app_protocol::requests::TimelineRecoveryResponse;
use talos_auth::RequestIdentity;
use talos_service::TimelineSubscriptionMessage;
use talos_types::{RecoverTimelineRequest, SessionActiveTurnRequest, SubscribeTimelineRequest};

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
        .app
        .open_timeline_subscription(
            &shared_actor(&actor.0),
            SubscribeTimelineRequest {
                session_id: session_id.clone(),
                since_event_id: after_event_id,
                replay_limit: Some(limit),
                close_when_idle: Some(true),
                close_on_terminal: Some(true),
            },
        )
        .await
        .map_err(map_event_replay_error)?;

    let stream = ReceiverStream::new(subscription.subscription.events).filter_map(|message| {
        let TimelineSubscriptionMessage::Event(record) = message else {
            return None;
        };
        Some(timeline_sse_event(record).map_err(io::Error::other))
    });

    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}

fn timeline_sse_event(
    record: talos_types::talos::TimelineEventEnvelope,
) -> Result<Event, serde_json::Error> {
    let event_name = canonical_method(&record.event.event_type);
    let event_id = record.event_id.clone();
    let is_live_metrics = matches!(
        record.event.payload,
        talos_types::talos::TalosTimelineEventPayload::TurnMetrics(_)
    );
    let data = serde_json::to_string(&record)?;
    let event = Event::default().event(event_name).data(data);
    if is_live_metrics {
        Ok(event)
    } else {
        Ok(event.id(event_id))
    }
}

pub(super) async fn recover_timeline_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<TimelineRecoveryQuery>,
) -> Result<Json<TimelineRecoveryResponse>, ApiError> {
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
    let response = state
        .gateway
        .recover_timeline(
            &shared_actor(&actor.0),
            RecoverTimelineRequest {
                session_id: session_id.clone(),
                after_event_id,
                replay_limit: Some(limit),
            },
        )
        .await
        .map_err(map_event_replay_error)?;

    Ok(Json(response))
}

fn parse_cursor(
    query_after_event_id: Option<String>,
    headers: &HeaderMap,
) -> Result<Option<String>, ApiError> {
    Ok(query_after_event_id.or_else(|| {
        headers
            .get("last-event-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    }))
}

fn map_event_replay_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::ResourceExpired(message) => {
            ApiError::new(StatusCode::GONE, "event_cursor_expired", message)
        }
        talos_error::TalosError::ValidationError(message) => {
            ApiError::new(StatusCode::BAD_REQUEST, "invalid_cursor", message)
        }
        talos_error::TalosError::ResourceNotFound(message) => {
            ApiError::new(StatusCode::NOT_FOUND, "session_not_found", message)
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
    let snapshot = state
        .gateway
        .get_active_turn(
            &shared_actor(&actor.0),
            SessionActiveTurnRequest { session_id },
        )
        .await
        .map_err(map_active_turn_error)?;

    let value = serde_json::to_value(snapshot).map_err(|err| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "active_turn_serialize_failed",
            format!("failed to serialize active turn snapshot: {err}"),
        )
    })?;
    Ok(Json(value))
}

fn map_active_turn_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
            StatusCode::NOT_FOUND,
            "session_not_found",
            "session not found",
        ),
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "active_turn_failed",
            other.to_string(),
        ),
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
