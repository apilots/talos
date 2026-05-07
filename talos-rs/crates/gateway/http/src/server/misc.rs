use std::sync::Arc;

use axum::extract::State;
use axum::response::Json;
use axum::Json as BodyJson;
use serde_json::{json, Value};
use tokio::signal;

use talos_config::LlmConfig;

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
        .config
        .list_models()
        .await
        .into_iter()
        .map(|model| json!(model))
        .collect();
    Ok(Json(json!({ "data": data })))
}

pub(super) async fn get_llm_handler(
    State(state): State<Arc<HttpApiState>>,
) -> Result<Json<LlmConfig>, ApiError> {
    let llm = state.config.get_llm().map_err(map_config_error)?;
    Ok(Json(llm))
}

pub(super) async fn put_llm_handler(
    State(state): State<Arc<HttpApiState>>,
    BodyJson(llm): BodyJson<LlmConfig>,
) -> Result<Json<LlmConfig>, ApiError> {
    let result = state.config.update_llm(llm).map_err(map_config_error)?;
    Ok(Json(result))
}

fn map_config_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::ValidationError(message) => ApiError::new(
            axum::http::StatusCode::BAD_REQUEST,
            "invalid_request",
            message,
        ),
        other => ApiError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            other.to_string(),
        ),
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
