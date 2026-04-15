use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use serde_json::{json, Value};
use tokio::signal;

use talos_auth::{AuthResult, TokenType};
use talos_types::talos::{TalosContext, TalosEventType, TalosInputItem};

use super::errors::ApiError;
use super::state::HttpApiState;

pub(super) async fn health_handler() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "service": "talos-gateway-http",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

pub(super) async fn list_models_handler(
    State(state): State<Arc<HttpApiState>>,
) -> Result<Json<Value>, ApiError> {
    let data: Vec<Value> = state.models.iter().map(|model| json!(model)).collect();
    Ok(Json(json!({ "data": data })))
}

pub(super) fn validate_text_input(input: &[TalosInputItem]) -> Result<(), ApiError> {
    let mut texts = Vec::new();
    for item in input {
        if let TalosInputItem::Text { text } = item {
            let normalized = text.trim();
            if !normalized.is_empty() {
                texts.push(normalized.to_string());
            }
        }
    }

    if texts.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "input must include at least one non-empty text item",
        ));
    }

    Ok(())
}

pub(super) fn validate_model_supported(state: &HttpApiState, model: &str) -> Result<(), ApiError> {
    if state.models.iter().any(|entry| entry.id == model) {
        return Ok(());
    }

    Err(ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        format!("unsupported model: {}", model),
    ))
}

pub(super) fn validate_auth_context(
    auth: &AuthResult,
    context: &TalosContext,
    required_scope: &str,
) -> Result<(), ApiError> {
    match auth.token_type {
        TokenType::User => {
            if auth.user_id != context.user_id {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    "user token mismatch",
                ));
            }
            Ok(())
        }
        TokenType::Service => {
            if let Some(act_as) = &auth.act_as {
                if act_as != "*" && act_as != &context.user_id {
                    return Err(ApiError::new(
                        StatusCode::FORBIDDEN,
                        "forbidden",
                        "act_as mismatch",
                    ));
                }
            } else {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    "service token requires act_as",
                ));
            }

            if !auth.allowed_channels.is_empty()
                && !auth
                    .allowed_channels
                    .iter()
                    .any(|v| v == "*" || context.channel_id.as_deref() == Some(v.as_str()))
            {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    "channel not allowed",
                ));
            }

            if !auth.scopes.is_empty()
                && !auth.scopes.iter().any(|v| v == "*" || v == required_scope)
            {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    "scope not allowed",
                ));
            }

            Ok(())
        }
    }
}

pub(super) fn talos_event_name(event_type: &TalosEventType) -> &'static str {
    match event_type {
        TalosEventType::MessageStarted => "message.started",
        TalosEventType::MessageReasoningDelta => "message.reasoning.delta",
        TalosEventType::MessageExecutionSummary => "message.execution.summary",
        TalosEventType::MessageOutputDelta => "message.output.delta",
        TalosEventType::MessageToolCallStarted => "message.tool_call.started",
        TalosEventType::MessageToolCallArgsDelta => "message.tool_call.args_delta",
        TalosEventType::MessageToolCallQueued => "message.tool_call.queued",
        TalosEventType::MessageToolCallExecuting => "message.tool_call.executing",
        TalosEventType::MessageToolCallProgressDelta => "message.tool_call.progress_delta",
        TalosEventType::MessageToolCallCompleted => "message.tool_call.completed",
        TalosEventType::MessageToolCallPendingApproval => "message.tool_call.pending_approval",
        TalosEventType::MessageCompleted => "message.completed",
        TalosEventType::MessageError => "message.error",
    }
}

/// Handle shutdown signals.
pub(super) async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
