use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tracing::error;
use uuid::Uuid;

use talos_auth::RequestIdentity;
use talos_gateway::GatewayRequest;
use talos_session_store::{
    ActiveTurnStatus, IdempotencyRecord, IdempotencyReservation, IdempotencyStatus,
    MessageCommandAck,
};
use talos_types::talos::{
    TalosContext, TalosEvent, TalosEventPayload, TalosEventType, TalosMessageErrorEvent,
    TalosMessageRequest,
};

use super::errors::ApiError;
use super::misc::validate_text_input;
use super::state::HttpApiState;

pub(super) async fn session_message_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(request): Json<TalosMessageRequest>,
) -> Result<Response, ApiError> {
    let ack = submit_message_command(state, actor.0, headers, session_id, request).await?;
    Ok((StatusCode::ACCEPTED, Json(ack)).into_response())
}

#[derive(Deserialize)]
pub(super) struct MessageRetryRequest {
    input: Vec<talos_types::talos::TalosInputItem>,
    workspace_id: Option<String>,
    client_message_id: String,
    retry_of_message_id: String,
}

pub(super) async fn retry_message_attempt_handler(
    State(state): State<Arc<HttpApiState>>,
    actor: axum::extract::Extension<RequestIdentity>,
    mut headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(request): Json<MessageRetryRequest>,
) -> Result<Response, ApiError> {
    let message_request = TalosMessageRequest {
        input: request.input.clone(),
        workspace_id: request.workspace_id.clone(),
    };
    let request_hash = message_request_hash(&session_id, &message_request)?;
    let idempotency_key = retry_idempotency_key(&request)?;
    let idempotency_key_string = idempotency_key
        .to_str()
        .map_err(|_| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_retry_request",
                "client_message_id and retry_of_message_id must be valid HTTP header values",
            )
        })?
        .to_string();
    if let Some(record) = state
        .idempotency_store
        .get(&session_id, &idempotency_key_string)
        .await
        .map_err(|err| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "idempotency_failed",
                err.to_string(),
            )
        })?
    {
        if record.request_hash != request_hash {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "idempotency_conflict",
                "Retry attempt was already used with a different request",
            ));
        }
        return Ok((
            StatusCode::ACCEPTED,
            Json(ack_from_idempotency_record(record)),
        )
            .into_response());
    }

    validate_retry_attempt(&state, &session_id, &request).await?;
    headers.insert("idempotency-key", idempotency_key);
    let ack = submit_message_command(state, actor.0, headers, session_id, message_request).await?;
    Ok((StatusCode::ACCEPTED, Json(ack)).into_response())
}

async fn validate_retry_attempt(
    state: &HttpApiState,
    session_id: &str,
    request: &MessageRetryRequest,
) -> Result<(), ApiError> {
    if request.client_message_id.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_retry_request",
            "client_message_id is required",
        ));
    }
    if request.retry_of_message_id.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_retry_request",
            "retry_of_message_id is required",
        ));
    }

    let Some(active_turn) = state
        .event_service
        .get_active_turn(session_id)
        .await
        .map_err(|err| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "active_turn_failed",
                err.to_string(),
            )
        })?
    else {
        return Ok(());
    };

    match active_turn.status {
        ActiveTurnStatus::Running
        | ActiveTurnStatus::WaitingUserAction
        | ActiveTurnStatus::Cancelling => Err(ApiError::new(
            StatusCode::CONFLICT,
            "retry_active_turn_running",
            "The original turn is still active; reconnect to the existing event stream instead.",
        )),
        ActiveTurnStatus::Completed => Err(ApiError::new(
            StatusCode::CONFLICT,
            "retry_source_completed",
            "The original turn has already completed; refresh session history instead.",
        )),
        ActiveTurnStatus::Failed => {
            if active_turn.message_id.as_deref() == Some(request.retry_of_message_id.as_str()) {
                Ok(())
            } else {
                Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "retry_source_mismatch",
                    "retry_of_message_id does not match the failed active turn.",
                ))
            }
        }
    }
}

fn retry_idempotency_key(request: &MessageRetryRequest) -> Result<HeaderValue, ApiError> {
    HeaderValue::from_str(&format!(
        "retry_{}_{}",
        request.client_message_id, request.retry_of_message_id
    ))
    .map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_retry_request",
            "client_message_id and retry_of_message_id must be valid HTTP header values",
        )
    })
}

fn ack_from_idempotency_record(record: IdempotencyRecord) -> MessageCommandAck {
    record.ack.unwrap_or_else(|| MessageCommandAck {
        request_id: format!("req_{}", Uuid::new_v4().simple()),
        message_id: record.message_id,
        session_id: record.session_id,
        turn_id: record.turn_id,
        status: record.status,
    })
}

pub(super) async fn submit_message_command(
    state: Arc<HttpApiState>,
    actor: RequestIdentity,
    headers: HeaderMap,
    session_id: String,
    request: TalosMessageRequest,
) -> Result<MessageCommandAck, ApiError> {
    let session = state
        .access_service
        .get_session(&actor, &session_id)
        .await
        .map_err(map_session_message_error)?;

    let context = TalosContext {
        source_type: session.context.source_type.clone(),
        provider: session.context.provider.clone(),
        user_id: session.context.user_id.clone(),
        message_id: session.context.message_id.clone(),
        trace_id: session.context.trace_id.clone(),
        channel_id: session.context.channel_id.clone(),
        thread_id: session.context.thread_id.clone(),
        project_root: session.context.project_root.clone(),
        environment: session.context.environment.clone(),
    };
    validate_text_input(&request.input)?;

    let request_hash = message_request_hash(&session_id, &request)?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let request_id = format!("req_{}", Uuid::new_v4().simple());
    let turn_id = format!("turn_{}", Uuid::new_v4().simple());
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    if let Some(key) = idempotency_key.as_ref() {
        let expires_at = now_ms.saturating_add(
            (state
                .base_config
                .agent
                .kernel
                .session_events
                .idempotency_key_ttl_hours as i64)
                * 60
                * 60
                * 1000,
        );
        match state
            .idempotency_store
            .reserve(
                &session_id,
                key,
                &request_hash,
                &turn_id,
                now_ms,
                Some(expires_at),
            )
            .await
            .map_err(|err| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "idempotency_failed",
                    err.to_string(),
                )
            })? {
            IdempotencyReservation::Reserved(_) => {}
            IdempotencyReservation::Existing(record) => {
                if record.request_hash != request_hash {
                    return Err(ApiError::new(
                        StatusCode::CONFLICT,
                        "idempotency_conflict",
                        "Idempotency-Key was already used with a different request",
                    ));
                }
                return Ok(record.ack.unwrap_or_else(|| MessageCommandAck {
                    request_id,
                    message_id: record.message_id,
                    session_id: record.session_id,
                    turn_id: record.turn_id,
                    status: record.status,
                }));
            }
        }
    }

    let gateway_request = GatewayRequest {
        session_id: session_id.clone(),
        workspace_id: request.workspace_id.clone(),
        input: request.input.clone(),
        context,
    };

    let gateway_response = state
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
    let mut stream_rx = gateway_response;
    let first_event_timeout = Duration::from_millis(
        state
            .base_config
            .agent
            .kernel
            .session_events
            .first_event_timeout_ms
            .max(1),
    );
    let first_event = match tokio::time::timeout(first_event_timeout, stream_rx.recv()).await {
        Ok(Some(event)) => event,
        Ok(None) => {
            return fail_message_command_without_first_event(
                state,
                &session_id,
                &turn_id,
                &request_id,
                idempotency_key.as_deref(),
                stream_rx,
                "message stream closed before first event",
            )
            .await;
        }
        Err(_) => {
            if let Err(err) = state.gateway.cancel_active_turn(&session_id).await {
                error!(
                    session_id = %session_id,
                    turn_id = %turn_id,
                    error = %err,
                    "failed to cancel active turn after first event timeout"
                );
            }
            return fail_message_command_without_first_event(
                state,
                &session_id,
                &turn_id,
                &request_id,
                idempotency_key.as_deref(),
                stream_rx,
                "timed out waiting for first message event",
            )
            .await;
        }
    };
    let message_id = first_event.message_id.clone();
    state
        .event_service
        .record_talos_event(Some(turn_id.clone()), first_event)
        .await
        .map_err(|err| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "event_record_failed",
                err.to_string(),
            )
        })?;
    spawn_event_forwarder(
        state.event_service.clone(),
        Some(turn_id.clone()),
        stream_rx,
    );

    let ack = MessageCommandAck {
        request_id,
        message_id: Some(message_id.clone()),
        session_id: session_id.clone(),
        turn_id,
        status: IdempotencyStatus::Accepted,
    };
    if let Some(key) = idempotency_key.as_ref() {
        state
            .idempotency_store
            .complete(
                &session_id,
                key,
                IdempotencyStatus::Accepted,
                Some(&message_id),
                &ack,
                chrono::Utc::now().timestamp_millis(),
            )
            .await
            .map_err(|err| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "idempotency_failed",
                    err.to_string(),
                )
            })?;
    }

    Ok(ack)
}

fn spawn_event_forwarder(
    event_service: Arc<talos_session_store::SessionEventService>,
    turn_id: Option<String>,
    mut stream_rx: mpsc::Receiver<TalosEvent>,
) {
    tokio::spawn(async move {
        while let Some(event) = stream_rx.recv().await {
            if let Err(err) = event_service
                .record_talos_event(turn_id.clone(), event)
                .await
            {
                error!(error = %err, "failed to persist session stream event");
            }
        }
    });
}

async fn fail_message_command_without_first_event(
    state: Arc<HttpApiState>,
    session_id: &str,
    turn_id: &str,
    request_id: &str,
    idempotency_key: Option<&str>,
    stream_rx: mpsc::Receiver<TalosEvent>,
    message: &str,
) -> Result<MessageCommandAck, ApiError> {
    let error_event = TalosEvent {
        event_type: TalosEventType::MessageError,
        message_id: format!("msg_{}", Uuid::new_v4().simple()),
        session_id: session_id.to_string(),
        sequence: 0,
        payload: TalosEventPayload::MessageError(TalosMessageErrorEvent {
            request_id: request_id.to_string(),
            error: message.to_string(),
        }),
    };
    let message_id = error_event.message_id.clone();
    if let Err(err) = state
        .event_service
        .record_talos_event(Some(turn_id.to_string()), error_event)
        .await
    {
        error!(
            session_id = %session_id,
            turn_id = %turn_id,
            error = %err,
            "failed to persist first-event timeout error event"
        );
    }
    spawn_event_forwarder(
        state.event_service.clone(),
        Some(turn_id.to_string()),
        stream_rx,
    );

    let ack = MessageCommandAck {
        request_id: request_id.to_string(),
        message_id: Some(message_id.clone()),
        session_id: session_id.to_string(),
        turn_id: turn_id.to_string(),
        status: IdempotencyStatus::FailedNoFirstEvent,
    };
    if let Some(key) = idempotency_key {
        if let Err(err) = state
            .idempotency_store
            .complete(
                session_id,
                key,
                IdempotencyStatus::FailedNoFirstEvent,
                Some(&message_id),
                &ack,
                chrono::Utc::now().timestamp_millis(),
            )
            .await
        {
            error!(
                session_id = %session_id,
                turn_id = %turn_id,
                idempotency_key = %key,
                error = %err,
                "failed to persist first-event timeout idempotency state"
            );
        }
    }

    Err(ApiError::new(
        StatusCode::GATEWAY_TIMEOUT,
        "first_event_timeout",
        message,
    ))
}

fn message_request_hash(
    session_id: &str,
    request: &TalosMessageRequest,
) -> Result<String, ApiError> {
    let canonical = serde_json::json!({
        "session_id": session_id,
        "workspace_id": &request.workspace_id,
        "input": &request.input,
    });
    let bytes = serde_json::to_vec(&canonical).map_err(|err| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("failed to serialize request hash: {}", err),
        )
    })?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
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
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
    Json(body): Json<ToolApprovalRequest>,
) -> impl IntoResponse {
    match state
        .access_service
        .submit_tool_approval(
            &actor.0,
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
    actor: axum::extract::Extension<RequestIdentity>,
    Path(session_id): Path<String>,
    Json(body): Json<DynamicToolResultRequest>,
) -> impl IntoResponse {
    match state
        .access_service
        .submit_dynamic_tool_result(
            &actor.0,
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

fn map_session_message_error(error: talos_error::TalosError) -> ApiError {
    match error {
        talos_error::TalosError::ResourceNotFound(_) => ApiError::new(
            StatusCode::NOT_FOUND,
            "invalid_request",
            "session not found",
        ),
        other => ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            other.to_string(),
        ),
    }
}
