use std::sync::Arc;

use axum::extract::{Extension, Path, State};
use axum::response::Json;
use axum::Json as BodyJson;
use serde_json::{json, Value};
use tokio::signal;

use talos_auth::RequestIdentity;
use talos_types::{
    ConfigGetRequest, ConfigPath, ConfigResponse, ConfigUpdateRequest, ConfigValue, LlmConfig,
    ModelListRequest, ModelListResponse,
};

use super::access::shared_actor;
use super::errors::{api_success, ApiError, JsonResponse};
use super::state::HttpApiState;

pub(super) async fn health_handler(State(state): State<Arc<HttpApiState>>) -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "service": "talos-gateway-http",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "auth_enabled": !state.auth_disabled,
        "cloud_auth_url": state.base_config.cloud.web_base_url,
    }))
}

pub(super) async fn list_models_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: Option<Extension<RequestIdentity>>,
) -> Result<JsonResponse<ModelListResponse>, ApiError> {
    let actor = actor
        .map(|Extension(identity)| identity)
        .unwrap_or_else(RequestIdentity::auth_disabled);
    let response = state
        .gateway
        .list_models(&shared_actor(&actor), ModelListRequest::default())
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(response))
}

pub(super) async fn get_llm_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: Option<Extension<RequestIdentity>>,
) -> Result<JsonResponse<ConfigResponse>, ApiError> {
    let actor = actor
        .map(|Extension(identity)| identity)
        .unwrap_or_else(RequestIdentity::auth_disabled);
    let llm = state
        .gateway
        .get_config(&shared_actor(&actor), ConfigGetRequest::default())
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(llm))
}

pub(super) async fn get_config_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: Option<Extension<RequestIdentity>>,
    Path(path): Path<String>,
) -> Result<JsonResponse<ConfigResponse>, ApiError> {
    let actor = actor
        .map(|Extension(identity)| identity)
        .unwrap_or_else(RequestIdentity::auth_disabled);
    let config = state
        .gateway
        .get_config(
            &shared_actor(&actor),
            ConfigGetRequest {
                path: parse_config_path(&path)?,
            },
        )
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(config))
}

pub(super) async fn put_config_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: Option<Extension<RequestIdentity>>,
    BodyJson(value): BodyJson<ConfigValue>,
) -> Result<JsonResponse<ConfigResponse>, ApiError> {
    let actor = actor
        .map(|Extension(identity)| identity)
        .unwrap_or_else(RequestIdentity::auth_disabled);
    let result = state
        .gateway
        .update_config(&shared_actor(&actor), ConfigUpdateRequest { value })
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(result))
}

pub(super) async fn put_llm_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: Option<Extension<RequestIdentity>>,
    BodyJson(llm): BodyJson<LlmConfig>,
) -> Result<JsonResponse<ConfigResponse>, ApiError> {
    let actor = actor
        .map(|Extension(identity)| identity)
        .unwrap_or_else(RequestIdentity::auth_disabled);
    let result = state
        .gateway
        .update_config(
            &shared_actor(&actor),
            ConfigUpdateRequest {
                value: ConfigValue::Llm(Box::new(llm)),
            },
        )
        .await
        .map_err(ApiError::from_talos_error)?;
    Ok(api_success(result))
}

fn parse_config_path(path: &str) -> Result<ConfigPath, ApiError> {
    match path {
        "llm" => Ok(ConfigPath::Llm),
        "webSearch" | "web-search" => Ok(ConfigPath::WebSearch),
        other => Err(ApiError::from_talos_error(talos_error::TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("unsupported config path: {}", other),
        ))),
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
