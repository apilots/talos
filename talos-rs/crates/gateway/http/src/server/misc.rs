use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use serde_json::{json, Value};
use tokio::signal;

use talos_types::talos::{TalosEventType, TalosInputItem};

use super::errors::ApiError;
use super::state::HttpApiState;

pub(super) async fn health_handler(State(state): State<Arc<HttpApiState>>) -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "service": "talos-gateway-http",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "auth_enabled": !state.auth_disabled,
    }))
}

pub(super) async fn list_models_handler(
    State(state): State<Arc<HttpApiState>>,
) -> Result<Json<Value>, ApiError> {
    let data: Vec<Value> = state
        .models
        .read()
        .await
        .iter()
        .map(|model| json!(model))
        .collect();
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
    if state
        .models
        .try_read()
        .map(|g| g.iter().any(|entry| entry.id == model))
        .unwrap_or(false)
    {
        return Ok(());
    }

    Err(ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        format!("unsupported model: {}", model),
    ))
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
