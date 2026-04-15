use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tracing::error;
use uuid::Uuid;

use talos_auth::AuthResult;
use talos_gateway::GatewayRequest;
use talos_types::talos::{
    TalosContext, TalosMessageRequest, TalosMessageResponse, TalosOutputItem,
};

use super::errors::ApiError;
use super::misc::{talos_event_name, validate_auth_context, validate_text_input};
use super::state::HttpApiState;

pub(super) async fn session_message_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<AuthResult>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(request): Json<TalosMessageRequest>,
) -> Result<Response, ApiError> {
    let stream = request.stream.unwrap_or(false);

    if stream {
        return stream_session_message(state, auth.0, session_id, request).await;
    }

    let response = process_session_message(state, auth.0, headers, session_id, request).await?;
    Ok(Json(response).into_response())
}

pub(super) async fn stream_session_message(
    state: Arc<HttpApiState>,
    auth: AuthResult,
    session_id: String,
    request: TalosMessageRequest,
) -> Result<Response, ApiError> {
    let Some(session) = state
        .session_service
        .get_session(&session_id)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                e.to_string(),
            )
        })?
    else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "invalid_request",
            "session not found",
        ));
    };

    let context = TalosContext {
        source_type: session.context.source_type.clone(),
        provider: session.context.provider.clone(),
        user_id: session.context.user_id.clone(),
        message_id: session.context.message_id.clone(),
        trace_id: session.context.trace_id.clone(),
        channel_id: session.context.channel_id.clone(),
        thread_id: session.context.thread_id.clone(),
        environment: session.context.environment.clone(),
    };
    validate_auth_context(&auth, &context, super::DEFAULT_SCOPE_MESSAGES_WRITE)?;
    validate_text_input(&request.input)?;
    let gateway_request = GatewayRequest {
        session_id: session_id.clone(),
        workspace_id: request.workspace_id.clone(),
        input: request.input.clone(),
        context,
    };

    let stream_rx = state
        .gateway
        .handle_request_stream(gateway_request)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                e.to_string(),
            )
        })?;
    let stream = ReceiverStream::new(stream_rx).map(|event| {
        let event_name = talos_event_name(&event.event_type);
        let data = match serde_json::to_string(&event) {
            Ok(data) => data,
            Err(err) => {
                error!(error = %err, "failed to serialize talos event");
                serde_json::json!({
                    "event_type": "error",
                    "label": "error",
                    "data": { "type": "none" },
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                })
                .to_string()
            }
        };
        Ok::<Event, Infallible>(Event::default().event(event_name).data(data))
    });
    let sse = Sse::new(stream).keep_alive(KeepAlive::default());
    Ok(sse.into_response())
}

pub(super) async fn process_session_message(
    state: Arc<HttpApiState>,
    auth: AuthResult,
    headers: HeaderMap,
    session_id: String,
    request: TalosMessageRequest,
) -> Result<TalosMessageResponse, ApiError> {
    let Some(session) = state
        .session_service
        .get_session(&session_id)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                e.to_string(),
            )
        })?
    else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "invalid_request",
            "session not found",
        ));
    };

    let context = TalosContext {
        source_type: session.context.source_type.clone(),
        provider: session.context.provider.clone(),
        user_id: session.context.user_id.clone(),
        message_id: session.context.message_id.clone(),
        trace_id: session.context.trace_id.clone(),
        channel_id: session.context.channel_id.clone(),
        thread_id: session.context.thread_id.clone(),
        environment: session.context.environment.clone(),
    };
    validate_auth_context(&auth, &context, super::DEFAULT_SCOPE_MESSAGES_WRITE)?;
    let idempotency_key = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|v| format!("{}:{}", session_id, v));

    if let Some(key) = &idempotency_key {
        if let Some(cached) = state.idempotency_cache.read().await.get(key) {
            return Ok(cached.clone());
        }
    }

    validate_text_input(&request.input)?;

    let gateway_request = GatewayRequest {
        session_id: session_id.clone(),
        workspace_id: request.workspace_id.clone(),
        input: request.input.clone(),
        context,
    };

    let gateway_response = state
        .gateway
        .handle_request(gateway_request)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                e.to_string(),
            )
        })?;

    let output = if gateway_response.output_items.is_empty() {
        vec![TalosOutputItem::OutputText {
            text: gateway_response.content.clone(),
        }]
    } else {
        gateway_response.output_items.clone()
    };

    let response = TalosMessageResponse {
        request_id: format!("req_{}", Uuid::new_v4().simple()),
        message_id: format!("msg_{}", Uuid::new_v4().simple()),
        session_id,
        model: gateway_response.model.clone(),
        output,
        reasoning: gateway_response.reasoning.clone(),
        stop_reason: gateway_response.stop_reason,
        usage: gateway_response.usage.clone(),
    };

    if let Some(key) = idempotency_key {
        state
            .idempotency_cache
            .write()
            .await
            .insert(key, response.clone());
    }

    Ok(response)
}

#[derive(Deserialize)]
pub(super) struct ToolApprovalRequest {
    call_id: String,
    tool_name: String,
    approved: bool,
    reason: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct DynamicToolResultRequest {
    call_id: String,
    tool_name: String,
    output: String,
    success: bool,
}

pub(super) async fn submit_tool_approval_handler(
    State(state): State<Arc<HttpApiState>>,
    Path(session_id): Path<String>,
    Json(body): Json<ToolApprovalRequest>,
) -> impl IntoResponse {
    match state
        .session_service
        .submit_tool_approval(
            &session_id,
            &body.call_id,
            &body.tool_name,
            body.approved,
            body.reason,
        )
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "approval_failed",
            e.to_string(),
        )
        .into_response(),
    }
}

pub(super) async fn submit_dynamic_tool_result_handler(
    State(state): State<Arc<HttpApiState>>,
    Path(session_id): Path<String>,
    Json(body): Json<DynamicToolResultRequest>,
) -> impl IntoResponse {
    match state
        .session_service
        .submit_dynamic_tool_result(
            &session_id,
            &body.call_id,
            &body.tool_name,
            body.output,
            body.success,
        )
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "dynamic_tool_result_failed",
            e.to_string(),
        )
        .into_response(),
    }
}
