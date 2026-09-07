use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use talos_app_protocol::events::{SubscriptionTerminatedNotification, TimelineEventNotification};
use talos_app_protocol::jsonrpc::JsonRpcNotification;
use talos_error::{ApiError, TalosError, TalosResult};
use talos_types::ApiResponse;
use tokio::sync::{oneshot, Mutex};

type PendingResponses = Arc<Mutex<HashMap<i32, oneshot::Sender<TalosResult<Value>>>>>;

pub(super) fn decode_subscription_terminated(
    notification: &JsonRpcNotification,
) -> TalosResult<Option<SubscriptionTerminatedNotification>> {
    if notification.method != "subscription/terminated" {
        return Ok(None);
    }
    let Some(params) = notification.params.clone() else {
        return Ok(None);
    };
    serde_json::from_value(params).map(Some).map_err(|error| {
        TalosError::internal_with_source(
            "App-server communication failed; technical details were withheld.",
            error,
        )
    })
}

pub(super) fn decode_event_notification(
    notification: JsonRpcNotification,
) -> TalosResult<Option<TimelineEventNotification>> {
    let Some(params) = notification.params else {
        return Ok(None);
    };
    if !notification.method.starts_with("timeline/") {
        return Ok(None);
    }
    serde_json::from_value(params).map(Some).map_err(|error| {
        TalosError::internal_with_source(
            "App-server communication failed; technical details were withheld.",
            error,
        )
    })
}

pub(super) fn decode_message_queue_changed(
    notification: &JsonRpcNotification,
) -> TalosResult<Option<talos_types::MessageQueueChangedNotification>> {
    if notification.method != "messageQueue/changed" {
        return Ok(None);
    }
    let Some(params) = notification.params.clone() else {
        return Ok(None);
    };
    serde_json::from_value(params).map(Some).map_err(|error| {
        TalosError::internal_with_source(
            "App-server communication failed; technical details were withheld.",
            error,
        )
    })
}

pub(super) fn map_app_server_error(
    error: talos_app_protocol::errors::AppServerErrorBody,
) -> TalosError {
    TalosError::internal_with_source(
        format!("App-server protocol error: {}", error.message),
        error,
    )
}

pub(super) fn decode_api_response(value: Value) -> TalosResult<Value> {
    let response: ApiResponse<Value> = serde_json::from_value(value).map_err(|error| {
        TalosError::internal_with_source(
            "App-server communication failed; technical details were withheld.",
            error,
        )
    })?;
    match response {
        ApiResponse::Success { data } => Ok(data),
        ApiResponse::Error { error } => Err(map_api_error(error)),
    }
}

fn map_api_error(error: ApiError) -> TalosError {
    error.into()
}

pub(super) async fn fail_pending_responses(pending_responses: &PendingResponses, message: String) {
    let pending = {
        let mut guard = pending_responses.lock().await;
        std::mem::take(&mut *guard)
    };
    for (_, sender) in pending {
        let _ = sender.send(Err(TalosError::internal(
            "App-server connection stopped before the request completed.",
        )
        .context(&message)));
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use serde_json::json;
    use talos_app_protocol::errors::{AppServerErrorBody, INVALID_PARAMS};
    use talos_error::ErrorCode;

    use super::{decode_api_response, map_app_server_error};

    #[test]
    fn decode_api_response_preserves_application_error_code() {
        let error = decode_api_response(json!({
            "status": "error",
            "error": {
                "code": "invalidCursor",
                "message": "Invalid event cursor",
                "localization": null
            }
        }))
        .expect_err("application error should be rejected");

        assert!(error.is_code(ErrorCode::InvalidCursor));
        assert_eq!(error.to_api_error().message, "Invalid event cursor");
    }

    #[test]
    fn protocol_error_preserves_typed_protocol_source() {
        let error = map_app_server_error(AppServerErrorBody::new(INVALID_PARAMS, "invalid params"));

        assert!(error.is_internal());
        let source = error.source().expect("protocol source should be preserved");
        let protocol = source
            .downcast_ref::<AppServerErrorBody>()
            .expect("source should remain a protocol error");
        assert_eq!(protocol.code, INVALID_PARAMS);
    }
}
