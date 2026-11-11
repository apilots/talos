use super::{event_type, item_type, ResponsesContract};
use serde_json::Value;
use talos_error::{ErrorCode, TalosError, TalosResult};

pub(super) fn compatible_finish_reason(status: Option<&str>) -> Option<String> {
    match status {
        Some("completed") => Some("stop".to_owned()),
        Some(status) if !status.is_empty() => Some(status.to_owned()),
        _ => None,
    }
}

pub(super) fn official_finish_reason(
    status: Option<&str>,
    error_code: Option<&str>,
    incomplete_reason: Option<&str>,
    has_tool_calls: bool,
) -> TalosResult<Option<String>> {
    match status {
        Some("completed") => Ok(Some("stop".to_string())),
        Some("incomplete") if incomplete_reason == Some("max_output_tokens") && !has_tool_calls => {
            Ok(Some("length".to_string()))
        }
        Some("cancelled") => Err(TalosError::new(
            ErrorCode::Cancelled,
            "The provider cancelled the model response.",
        )),
        Some("failed" | "incomplete") => Err(crate::error::stream_response_error(
            error_code.or(incomplete_reason),
        )),
        Some("queued" | "in_progress") => Err(TalosError::internal(
            "The model response has not reached a terminal state.",
        )),
        Some(status) if !status.is_empty() => Ok(Some(status.to_string())),
        _ => Ok(None),
    }
}

pub(super) fn stream_terminal(
    event: &Value,
    has_tool_calls: bool,
    contract: ResponsesContract,
) -> Option<TalosResult<String>> {
    let status = match event_type(event) {
        "response.completed" => "completed",
        "response.failed" | "error" => "failed",
        "response.incomplete" => "incomplete",
        "response.cancelled" if contract == ResponsesContract::Official => "cancelled",
        _ => return None,
    };
    let response = &event["response"];
    let nested_error = event["error"]["code"]
        .as_str()
        .or_else(|| response["error"]["code"].as_str());
    let incomplete_reason = response["incomplete_details"]["reason"].as_str();

    if contract == ResponsesContract::Compatible {
        return Some(if status == "completed" {
            Ok(compatible_finish_reason(response["status"].as_str())
                .unwrap_or_else(|| "stop".to_owned()))
        } else {
            Err(crate::error::stream_response_error(
                nested_error.or(incomplete_reason),
            ))
        });
    }

    let has_tool_calls = has_tool_calls
        || response["output"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item_type(item) == "function_call"));
    Some(
        official_finish_reason(
            Some(status),
            event["code"].as_str().or(nested_error),
            incomplete_reason,
            has_tool_calls,
        )
        .map(|reason| reason.expect("terminal Responses status has a finish reason")),
    )
}

pub(super) fn unterminated_stream(request_context: &str) -> TalosError {
    TalosError::internal("The model response stream ended before a terminal event.")
        .context(request_context)
}
