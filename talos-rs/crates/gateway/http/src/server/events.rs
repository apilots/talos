use std::io;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderMap, HeaderValue};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use talos_agent_api::{RecoverTimelineResponse, SessionActiveTurnResponse, TimelineEvent};
use talos_app_protocol::events::timeline_event_method;
use talos_app_protocol::events::{SubscriptionTerminatedEvent, SubscriptionTerminatedReason};
use talos_auth::RequestIdentity;
use talos_service::{
    TimelineSubscriptionCloseReason, TimelineSubscriptionClosed, TimelineSubscriptionMessage,
};
use talos_types::{RecoverTimelineRequest, SessionActiveTurnRequest, SubscribeTimelineRequest};

use super::access::shared_actor;
use super::errors::{api_success, ApiError, JsonResponse};
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
        .unwrap_or(state.base_config.agent.timeline_replay_batch_size)
        .max(1);

    let subscription = state
        .app
        .open_timeline_subscription(
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

    let stream = ReceiverStream::new(subscription.events).map(|message| {
        match message {
            TimelineSubscriptionMessage::Event(record) => timeline_sse_event(*record),
            TimelineSubscriptionMessage::MessageQueueChanged(change) => {
                message_queue_changed_sse_event(change)
            }
            TimelineSubscriptionMessage::Closed(closed) => {
                timeline_subscription_closed_sse_event(closed)
            }
        }
        .map_err(io::Error::other)
    });

    Ok(timeline_sse_response(
        Sse::new(stream)
            .keep_alive(KeepAlive::default())
            .into_response(),
    ))
}

fn message_queue_changed_sse_event(
    change: talos_types::MessageQueueChangedNotification,
) -> Result<Event, serde_json::Error> {
    Ok(Event::default()
        .event("messageQueue/changed")
        .data(serde_json::to_string(&change)?))
}

fn timeline_sse_response(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

fn timeline_sse_event(record: TimelineEvent) -> Result<Event, serde_json::Error> {
    let event_name = timeline_event_method(&record.event.payload);
    let event_id = record.event_id.clone();
    let data = serde_json::to_string(&record)?;
    Ok(Event::default().event(event_name).id(event_id).data(data))
}

pub(super) async fn recover_timeline_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<TimelineRecoveryQuery>,
) -> Result<JsonResponse<RecoverTimelineResponse>, ApiError> {
    let after_event_id = parse_cursor(query.after_event_id, &headers)?;
    let limit = query
        .limit
        .unwrap_or(state.base_config.agent.timeline_replay_batch_size)
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

    Ok(api_success(response))
}

fn timeline_subscription_closed_sse_event(
    closed: TimelineSubscriptionClosed,
) -> Result<Event, serde_json::Error> {
    let data = serde_json::to_string(&SubscriptionTerminatedEvent {
        reason: map_subscription_close_reason(closed.reason),
        message: closed.message,
    })?;
    Ok(Event::default().event("subscription/terminated").data(data))
}

fn map_subscription_close_reason(
    reason: TimelineSubscriptionCloseReason,
) -> SubscriptionTerminatedReason {
    match reason {
        TimelineSubscriptionCloseReason::Completed => SubscriptionTerminatedReason::Completed,
        TimelineSubscriptionCloseReason::ServerShutdown => {
            SubscriptionTerminatedReason::ServerShutdown
        }
        TimelineSubscriptionCloseReason::Lagged => SubscriptionTerminatedReason::Lagged,
    }
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
    match error.error_code() {
        talos_error::ErrorCode::ResourceExpired => {
            ApiError::new(talos_error::ErrorCode::EventCursorExpired)
        }
        talos_error::ErrorCode::InvalidRequest => {
            ApiError::new(talos_error::ErrorCode::InvalidCursor)
        }
        talos_error::ErrorCode::ResourceNotFound => {
            ApiError::new(talos_error::ErrorCode::SessionNotFound)
        }
        _ => ApiError::from_talos_error(error),
    }
}

pub(super) async fn get_active_turn_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> Result<JsonResponse<SessionActiveTurnResponse>, ApiError> {
    let snapshot = state
        .gateway
        .get_active_turn(
            &shared_actor(&actor.0),
            SessionActiveTurnRequest { session_id },
        )
        .await
        .map_err(map_active_turn_error)?;

    Ok(api_success(snapshot))
}

fn map_active_turn_error(error: talos_error::TalosError) -> ApiError {
    match error.error_code() {
        talos_error::ErrorCode::ResourceNotFound => {
            ApiError::new(talos_error::ErrorCode::SessionNotFound)
        }
        _ => ApiError::from_talos_error(error),
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
