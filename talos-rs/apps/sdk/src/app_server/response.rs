use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use talos_app_protocol::events::{
    SubscriptionTerminatedEnvelope, TimelineSubscriptionNotification,
};
use talos_app_protocol::jsonrpc::JsonRpcNotification;
use talos_error::{TalosError, TalosResult};
use tokio::sync::{oneshot, Mutex};

type PendingResponses = Arc<Mutex<HashMap<i32, oneshot::Sender<TalosResult<Value>>>>>;

pub(super) fn decode_subscription_terminated(
    notification: &JsonRpcNotification,
) -> TalosResult<Option<SubscriptionTerminatedEnvelope>> {
    if notification.method != "subscription.terminated" {
        return Ok(None);
    }
    let Some(params) = notification.params.clone() else {
        return Ok(None);
    };
    serde_json::from_value(params)
        .map(Some)
        .map_err(|error| TalosError::SerializationError(error.to_string()))
}

pub(super) fn decode_event_notification(
    notification: JsonRpcNotification,
) -> TalosResult<Option<TimelineSubscriptionNotification>> {
    let Some(params) = notification.params else {
        return Ok(None);
    };
    if !notification.method.starts_with("timeline.") {
        return Ok(None);
    }
    serde_json::from_value(params)
        .map(Some)
        .map_err(|error| TalosError::SerializationError(error.to_string()))
}

pub(super) fn map_app_server_error(
    error: talos_app_protocol::errors::AppServerErrorBody,
) -> TalosError {
    use talos_app_protocol::errors::{
        ACTIVE_TURN_RUNNING, CANCELLED, CONFLICT, FORBIDDEN, INVALID_PARAMS, NOT_FOUND,
        REQUEST_TIMEOUT, RESOURCE_EXPIRED, RUNTIME_UNAVAILABLE, TRANSPORT_UNAVAILABLE,
        UNAUTHORIZED, VALIDATION_FAILED,
    };

    match error.code {
        UNAUTHORIZED => TalosError::AuthError(error.message),
        FORBIDDEN => TalosError::AuthorizationDenied,
        NOT_FOUND => TalosError::ResourceNotFound(error.message),
        ACTIVE_TURN_RUNNING => TalosError::ActiveTurnRunning(error.message),
        CONFLICT => TalosError::Conflict(error.message),
        VALIDATION_FAILED | INVALID_PARAMS => TalosError::ValidationError(error.message),
        RESOURCE_EXPIRED => TalosError::ResourceExpired(error.message),
        RUNTIME_UNAVAILABLE => TalosError::RuntimeUnavailable(error.message),
        TRANSPORT_UNAVAILABLE => TalosError::TransportUnavailable(error.message),
        REQUEST_TIMEOUT => TalosError::Timeout(error.message),
        CANCELLED => TalosError::Cancelled(error.message),
        _ => TalosError::ClientError(error.message),
    }
}

pub(super) async fn fail_pending_responses(pending_responses: &PendingResponses, message: String) {
    let pending = {
        let mut guard = pending_responses.lock().await;
        std::mem::take(&mut *guard)
    };
    for (_, sender) in pending {
        let _ = sender.send(Err(TalosError::ClientError(message.clone())));
    }
}
