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
use super::errors::{ApiError, HttpApiErrorCode};
use super::state::HttpApiState;

#[derive(Debug, Deserialize)]
pub(super) struct TimelineEventsQuery {
    after_event_id: Option<String>,
    limit: Option<usize>,
    turn_id: Option<String>,
    close_when_idle: Option<bool>,
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
    let close_on_terminal = query.turn_id.is_some();
    let close_when_idle = query.close_when_idle.unwrap_or(true);
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
        .timeline_subscription_source(
            &shared_actor(&actor.0),
            SubscribeTimelineRequest {
                session_id: session_id.clone(),
                turn_id: query.turn_id,
                since_event_id: after_event_id,
                replay_limit: Some(limit),
                close_when_idle: Some(close_when_idle),
                close_on_terminal: Some(close_on_terminal),
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
        talos_types::talos::AgentEventPayload::TurnMetrics(_)
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
        talos_error::TalosError::ResourceExpired(message) => ApiError::new(
            StatusCode::GONE,
            HttpApiErrorCode::EventCursorExpired,
            message,
        ),
        talos_error::TalosError::ValidationError(message) => ApiError::new(
            StatusCode::BAD_REQUEST,
            HttpApiErrorCode::InvalidCursor,
            message,
        ),
        talos_error::TalosError::ResourceNotFound(message) => ApiError::new(
            StatusCode::NOT_FOUND,
            HttpApiErrorCode::SessionNotFound,
            message,
        ),
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            HttpApiErrorCode::EventReplayFailed,
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
            HttpApiErrorCode::ActiveTurnSerializeFailed,
            format!("failed to serialize active turn snapshot: {err}"),
        )
    })?;
    Ok(Json(value))
}

fn map_active_turn_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
            StatusCode::NOT_FOUND,
            HttpApiErrorCode::SessionNotFound,
            "session not found",
        ),
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            HttpApiErrorCode::ActiveTurnFailed,
            other.to_string(),
        ),
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
