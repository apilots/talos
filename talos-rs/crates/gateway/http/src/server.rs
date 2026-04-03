use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::signal;
use tokio::sync::RwLock;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tower_http::cors::CorsLayer;
use tracing::{error, info};
use uuid::Uuid;

use talos_auth::{Auth, AuthResult, JwtAuth, TokenType};
use talos_error::TalosResult;
use talos_gateway::{Gateway, GatewayRequest};
use talos_types::talos::{
    TalosContext, TalosEventType, TalosInputItem, TalosMessageRequest, TalosMessageResponse,
    TalosOutputItem,
};
use talos_types::{ModelInfo, SessionPatch, SessionQuery, SessionService};

const DEFAULT_SCOPE_MESSAGES_WRITE: &str = "messages:write";
const DEFAULT_SCOPE_SESSIONS_WRITE: &str = "sessions:write";

#[derive(Debug, Serialize)]
struct ApiErrorBody {
    code: String,
    message: String,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    body: ApiErrorBody,
}

impl ApiError {
    fn new(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            body: ApiErrorBody {
                code: code.into(),
                message: message.into(),
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

#[derive(Clone)]
struct HttpApiState {
    gateway: Arc<dyn Gateway>,
    session_service: Arc<dyn SessionService>,
    auth: Arc<JwtAuth>,
    static_token: Option<String>,
    auth_disabled: bool,
    idempotency_cache: Arc<RwLock<HashMap<String, TalosMessageResponse>>>,
    models: Arc<Vec<ModelInfo>>,
}

impl HttpApiState {
    fn new(
        gateway: Arc<dyn Gateway>,
        session_service: Arc<dyn SessionService>,
        jwt_secret: String,
        static_token: Option<String>,
        auth_disabled: bool,
        models: Vec<ModelInfo>,
    ) -> Self {
        Self {
            gateway,
            session_service,
            auth: Arc::new(JwtAuth::new(&jwt_secret)),
            static_token,
            auth_disabled,
            idempotency_cache: Arc::new(RwLock::new(HashMap::new())),
            models: Arc::new(models),
        }
    }
}

/// HTTP Server for the Talos Gateway.
pub struct HttpServer {
    gateway: Arc<dyn Gateway>,
    session_service: Arc<dyn SessionService>,
    host: String,
    port: u16,
    jwt_secret: String,
    static_token: Option<String>,
    auth_disabled: bool,
    models: Vec<ModelInfo>,
}

impl HttpServer {
    /// Create a new HTTP server instance.
    pub fn new(
        gateway: Arc<dyn Gateway>,
        session_service: Arc<dyn SessionService>,
        host: impl Into<String>,
        port: u16,
        _default_model: impl Into<String>,
        models: Vec<ModelInfo>,
    ) -> Self {
        Self {
            gateway,
            session_service,
            host: host.into(),
            port,
            jwt_secret: String::new(),
            static_token: None,
            auth_disabled: false,
            models,
        }
    }

    /// Override JWT secret used by the HTTP auth middleware.
    pub fn with_jwt_secret(mut self, jwt_secret: impl Into<String>) -> Self {
        self.jwt_secret = jwt_secret.into();
        self
    }

    /// Set a static bearer token used for deterministic integration tests.
    pub fn with_static_token(mut self, token: impl Into<String>) -> Self {
        self.static_token = Some(token.into());
        self
    }

    /// Disable auth checks for local mock mode.
    pub fn with_auth_disabled(mut self, disabled: bool) -> Self {
        self.auth_disabled = disabled;
        self
    }

    /// Start the HTTP server.
    pub async fn run(&self) -> TalosResult<()> {
        info!("Starting HTTP server on {}:{}", self.host, self.port);

        let state = Arc::new(HttpApiState::new(
            self.gateway.clone(),
            self.session_service.clone(),
            self.jwt_secret.clone(),
            self.static_token.clone(),
            self.auth_disabled,
            self.models.clone(),
        ));

        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/v1/models", get(list_models_handler))
            .route(
                "/v1/sessions",
                get(list_sessions_handler).post(create_session_handler),
            )
            .route(
                "/v1/sessions/:session_id",
                get(get_session_handler)
                    .patch(patch_session_handler)
                    .delete(close_session_handler),
            )
            .route(
                "/v1/sessions/:session_id/messages",
                post(session_message_handler),
            )
            .route(
                "/v1/sessions/:session_id/tool-approvals",
                post(submit_tool_approval_handler),
            )
            .route(
                "/v1/sessions/:session_id/dynamic-tool-results",
                post(submit_dynamic_tool_result_handler),
            )
            .with_state(state.clone())
            .layer(middleware::from_fn_with_state(state, auth_middleware))
            .layer(CorsLayer::permissive());

        let listener = tokio::net::TcpListener::bind(self.listen_addr())
            .await
            .map_err(|e| {
                error!("Failed to bind to {}:{}: {}", self.host, self.port, e);
                talos_error::TalosError::NetworkError(e.to_string())
            })?;

        info!("HTTP server listening on {}:{}", self.host, self.port);

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .map_err(|e| {
                error!("Server error: {}", e);
                talos_error::TalosError::NetworkError(e.to_string())
            })?;

        Ok(())
    }

    fn listen_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

async fn auth_middleware(
    State(state): State<Arc<HttpApiState>>,
    mut req: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path().to_string();
    if path == "/health" {
        return Ok(next.run(req).await);
    }

    if state.auth_disabled {
        req.extensions_mut().insert(mock_auth_result("mock-auth"));
        return Ok(next.run(req).await);
    }

    let auth_header = req.headers().get(axum::http::header::AUTHORIZATION);
    let token = match auth_header.and_then(|v| v.to_str().ok()) {
        Some(v) if v.starts_with("Bearer ") => v[7..].to_string(),
        _ => return Err(StatusCode::UNAUTHORIZED),
    };

    let auth_result = if state
        .static_token
        .as_ref()
        .is_some_and(|configured| configured == &token)
    {
        mock_auth_result("static-token")
    } else {
        state
            .auth
            .verify(&token)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?
    };

    if !auth_result.is_valid {
        return Err(StatusCode::UNAUTHORIZED);
    }

    req.extensions_mut().insert(auth_result);
    Ok(next.run(req).await)
}

fn mock_auth_result(user_id: &str) -> AuthResult {
    AuthResult {
        user_id: user_id.to_string(),
        expires_at: u64::MAX,
        is_valid: true,
        token_type: TokenType::Service,
        scopes: vec!["*".to_string()],
        allowed_channels: vec!["*".to_string()],
        act_as: Some("*".to_string()),
        issuer: Some("mock-auth".to_string()),
    }
}

async fn health_handler() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "service": "talos-gateway-http",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

async fn list_models_handler(
    State(state): State<Arc<HttpApiState>>,
) -> Result<Json<Value>, ApiError> {
    let data: Vec<Value> = state.models.iter().map(|model| json!(model)).collect();
    Ok(Json(json!({ "data": data })))
}

#[derive(Debug, Deserialize)]
struct CreateSessionRequest {
    context: TalosContext,
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PatchSessionRequest {
    #[serde(default)]
    conf: HashMap<String, Value>,
}

async fn create_session_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<AuthResult>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<Json<Value>, ApiError> {
    validate_auth_context(&auth.0, &request.context, DEFAULT_SCOPE_SESSIONS_WRITE)?;
    if let Some(model) = request.model.as_deref() {
        validate_model_supported(&state, &model)?;
    }

    let session = state
        .session_service
        .create_session(request.context, request.model)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                e.to_string(),
            )
        })?;

    Ok(Json(json!(session)))
}

async fn list_sessions_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<AuthResult>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<Value>, ApiError> {
    let sessions = state
        .session_service
        .list_sessions(query)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                e.to_string(),
            )
        })?;
    let data: Vec<Value> = sessions.into_iter().map(|s| json!(s)).collect();

    Ok(Json(json!({ "data": data })))
}

async fn get_session_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<AuthResult>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
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

    Ok(Json(json!(session)))
}

async fn patch_session_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<AuthResult>,
    Path(session_id): Path<String>,
    Json(request): Json<PatchSessionRequest>,
) -> Result<Json<Value>, ApiError> {
    let patch = SessionPatch { conf: request.conf };
    if let Some(model) = patch
        .agent_model()
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "invalid_request", e.to_string()))?
    {
        validate_model_supported(&state, &model)?;
    }
    let ack = state
        .session_service
        .patch_session(&session_id, patch)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                e.to_string(),
            )
        })?;

    if !ack.success {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "invalid_request",
            "session not found",
        ));
    }

    Ok(Json(json!({ "success": true })))
}

async fn close_session_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<AuthResult>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    state
        .session_service
        .close_session(&session_id)
        .await
        .map_err(|err| match err {
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
        })?;
    Ok(Json(json!({ "success": true })))
}

async fn session_message_handler(
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

async fn stream_session_message(
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
    };
    validate_auth_context(&auth, &context, DEFAULT_SCOPE_MESSAGES_WRITE)?;
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
                json!({
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

async fn process_session_message(
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
    };
    validate_auth_context(&auth, &context, DEFAULT_SCOPE_MESSAGES_WRITE)?;
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
        stop_reason: if gateway_response.success {
            gateway_response.stop_reason.clone()
        } else {
            "error".to_string()
        },
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
struct ToolApprovalRequest {
    call_id: String,
    tool_name: String,
    approved: bool,
    reason: Option<String>,
}

#[derive(Deserialize)]
struct DynamicToolResultRequest {
    call_id: String,
    tool_name: String,
    output: String,
    success: bool,
}

async fn submit_tool_approval_handler(
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

async fn submit_dynamic_tool_result_handler(
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

fn validate_text_input(input: &[TalosInputItem]) -> Result<(), ApiError> {
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

fn validate_model_supported(state: &HttpApiState, model: &str) -> Result<(), ApiError> {
    if state.models.iter().any(|entry| entry.id == model) {
        return Ok(());
    }

    Err(ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        format!("unsupported model: {}", model),
    ))
}

fn validate_auth_context(
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

fn talos_event_name(event_type: &TalosEventType) -> &'static str {
    match event_type {
        TalosEventType::MessageStarted => "message.started",
        TalosEventType::MessageReasoningDelta => "message.reasoning.delta",
        TalosEventType::MessageExecutionSummary => "message.execution.summary",
        TalosEventType::MessageOutputDelta => "message.output.delta",
        TalosEventType::MessageToolCallStarted => "message.tool_call.started",
        TalosEventType::MessageToolCallCompleted => "message.tool_call.completed",
        TalosEventType::MessageToolCallPendingApproval => "message.tool_call.pending_approval",
        TalosEventType::MessageCompleted => "message.completed",
        TalosEventType::MessageError => "message.error",
    }
}

/// Handle shutdown signals.
async fn shutdown_signal() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use talos_types::talos::{
        TalosEvent, TalosEventPayload, TalosMessageCompletedEvent, TalosMessageOutputDeltaEvent,
        TalosMessageStartedEvent, TalosProvider, TalosReasoning, TalosSourceType, TalosUsage,
    };
    use tower::util::ServiceExt;

    struct MockGateway;
    struct MockSessionService;

    fn test_model_info() -> ModelInfo {
        ModelInfo {
            id: "qwen-plus".to_string(),
            name: "Qwen Plus".to_string(),
            provider: "bailian".to_string(),
            context_window: 32_768,
            max_tokens: 8_192,
            reasoning: true,
            input: vec![talos_types::ModelInputKind::Text],
            cost: talos_types::ModelCostConfig::default(),
            compat: talos_types::ModelCompatConfig::default(),
        }
    }

    #[async_trait::async_trait]
    impl Gateway for MockGateway {
        async fn handle_request(
            &self,
            _request: GatewayRequest,
        ) -> TalosResult<talos_gateway::GatewayResponse> {
            Ok(talos_gateway::GatewayResponse {
                content: "mock content".to_string(),
                output_items: vec![TalosOutputItem::OutputText {
                    text: "mock content".to_string(),
                }],
                reasoning: vec![TalosReasoning {
                    text: "mock reasoning".to_string(),
                }],
                stop_reason: "end_turn".to_string(),
                usage: TalosUsage::default(),
                success: true,
                model: "qwen-plus".to_string(),
                session_id: "mock-session".to_string(),
            })
        }

        async fn handle_request_stream(
            &self,
            _request: GatewayRequest,
        ) -> TalosResult<tokio::sync::mpsc::Receiver<TalosEvent>> {
            let (tx, rx) = tokio::sync::mpsc::channel(8);
            tokio::spawn(async move {
                let started = TalosEvent {
                    event_type: TalosEventType::MessageStarted,
                    message_id: "msg_mock".to_string(),
                    session_id: "mock-session".to_string(),
                    sequence: 0,
                    payload: TalosEventPayload::MessageStarted(TalosMessageStartedEvent {
                        request_id: "req_mock".to_string(),
                        model: "qwen-plus".to_string(),
                        usage: TalosUsage::default(),
                        iteration: 0,
                        step_id: "start".to_string(),
                    }),
                };
                let _ = tx.send(started).await;

                let output = TalosEvent {
                    event_type: TalosEventType::MessageOutputDelta,
                    message_id: "msg_mock".to_string(),
                    session_id: "mock-session".to_string(),
                    sequence: 1,
                    payload: TalosEventPayload::MessageOutputDelta(TalosMessageOutputDeltaEvent {
                        request_id: "req_mock".to_string(),
                        delta: "stream only content".to_string(),
                        item_index: 0,
                        iteration: 1,
                        step_id: "step-1".to_string(),
                    }),
                };
                let _ = tx.send(output).await;

                let completed = TalosEvent {
                    event_type: TalosEventType::MessageCompleted,
                    message_id: "msg_mock".to_string(),
                    session_id: "mock-session".to_string(),
                    sequence: 2,
                    payload: TalosEventPayload::MessageCompleted(TalosMessageCompletedEvent {
                        request_id: "req_mock".to_string(),
                        stop_reason: "end_turn".to_string(),
                        usage: TalosUsage::default(),
                        total_output_items: 1,
                        iteration: 2,
                        step_id: "completed".to_string(),
                        task_count: 1,
                    }),
                };
                let _ = tx.send(completed).await;
            });
            Ok(rx)
        }

        async fn start(&self) -> TalosResult<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl SessionService for MockSessionService {
        async fn create_session(
            &self,
            context: TalosContext,
            model: Option<String>,
        ) -> TalosResult<talos_types::TalosSession> {
            Ok(talos_types::TalosSession {
                context: talos_types::SessionContext {
                    session_id: "mock-session".to_string(),
                    workspace_id: None,
                    source_type: context.source_type,
                    provider: context.provider,
                    user_id: context.user_id,
                    message_id: context.message_id,
                    trace_id: context.trace_id,
                    channel_id: context.channel_id,
                    thread_id: context.thread_id,
                },
                config: {
                    let mut config = talos_types::TalosConfig::default();
                    let resolved = model.unwrap_or_else(|| "qwen-plus".to_string());
                    config.agent.model = resolved.clone();
                    config.llm.model = resolved;
                    config
                },
                state: talos_types::SessionState::default(),
            })
        }

        async fn list_sessions(
            &self,
            _query: talos_types::SessionQuery,
        ) -> TalosResult<Vec<talos_types::TalosSession>> {
            Ok(Vec::new())
        }

        async fn get_session(
            &self,
            session_id: &str,
        ) -> TalosResult<Option<talos_types::TalosSession>> {
            if session_id != "mock-session" {
                return Ok(None);
            }
            Ok(Some(talos_types::TalosSession {
                context: talos_types::SessionContext {
                    session_id: "mock-session".to_string(),
                    workspace_id: None,
                    source_type: TalosSourceType::Http,
                    provider: TalosProvider::Http,
                    user_id: "static-token".to_string(),
                    message_id: "message-1".to_string(),
                    trace_id: "trace-1".to_string(),
                    channel_id: Some("test-channel".to_string()),
                    thread_id: None,
                },
                config: {
                    let mut config = talos_types::TalosConfig::default();
                    config.agent.model = "qwen-plus".to_string();
                    config.llm.model = "qwen-plus".to_string();
                    config
                },
                state: talos_types::SessionState {
                    created_at: 0,
                    last_activity: 0,
                    message_count: 1,
                    usage: talos_types::talos::TalosUsage::default(),
                },
            }))
        }

        async fn patch_session(
            &self,
            _session_id: &str,
            _patch: talos_types::SessionPatch,
        ) -> TalosResult<talos_types::SessionPatchAck> {
            Ok(talos_types::SessionPatchAck { success: true })
        }

        async fn close_session(&self, _session_id: &str) -> TalosResult<()> {
            Ok(())
        }

        async fn submit_tool_approval(
            &self,
            _session_id: &str,
            _call_id: &str,
            _tool_name: &str,
            _approved: bool,
            _reason: Option<String>,
        ) -> TalosResult<()> {
            Ok(())
        }

        async fn submit_dynamic_tool_result(
            &self,
            _session_id: &str,
            _call_id: &str,
            _tool_name: &str,
            _output: String,
            _success: bool,
        ) -> TalosResult<()> {
            Ok(())
        }
    }

    fn test_auth() -> AuthResult {
        AuthResult {
            user_id: "static-token".to_string(),
            expires_at: u64::MAX,
            is_valid: true,
            token_type: TokenType::Service,
            scopes: vec!["*".to_string()],
            allowed_channels: vec!["*".to_string()],
            act_as: Some("*".to_string()),
            issuer: Some("static-token".to_string()),
        }
    }

    #[tokio::test]
    async fn test_health() {
        let app = Router::new().route("/health", get(health_handler));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_models() {
        let state = Arc::new(HttpApiState::new(
            Arc::new(MockGateway),
            Arc::new(MockSessionService),
            "test-secret".to_string(),
            None,
            false,
            vec![test_model_info()],
        ));
        let app = Router::new()
            .route("/v1/models", get(list_models_handler))
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
        assert_eq!(payload["data"][0]["id"], "qwen-plus");
        assert_eq!(payload["data"][0]["provider"], "bailian");
        assert_eq!(payload["data"][0]["context_window"], 32_768);
        assert_eq!(payload["data"][0]["reasoning"], true);
    }

    #[test]
    fn test_http_server_listen_addr_uses_configured_host() {
        let server = HttpServer::new(
            Arc::new(MockGateway),
            Arc::new(MockSessionService),
            "127.0.0.1",
            8080,
            "qwen-plus",
            vec![test_model_info()],
        );

        assert_eq!(server.listen_addr(), "127.0.0.1:8080");
    }

    #[tokio::test]
    async fn test_models_require_bearer_when_auth_enabled() {
        let state = Arc::new(HttpApiState::new(
            Arc::new(MockGateway),
            Arc::new(MockSessionService),
            "test-secret".to_string(),
            None,
            false,
            vec![test_model_info()],
        ));
        let app = Router::new()
            .route("/v1/models", get(list_models_handler))
            .with_state(state.clone())
            .layer(middleware::from_fn_with_state(state, auth_middleware));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_models_allow_no_token_when_auth_disabled() {
        let state = Arc::new(HttpApiState::new(
            Arc::new(MockGateway),
            Arc::new(MockSessionService),
            "test-secret".to_string(),
            None,
            true,
            vec![test_model_info()],
        ));
        let app = Router::new()
            .route("/v1/models", get(list_models_handler))
            .with_state(state.clone())
            .layer(middleware::from_fn_with_state(state, auth_middleware));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_process_session_message() {
        let state = Arc::new(HttpApiState::new(
            Arc::new(MockGateway),
            Arc::new(MockSessionService),
            "test-secret".to_string(),
            Some("test-token".to_string()),
            false,
            vec![test_model_info()],
        ));

        let request = TalosMessageRequest {
            input: vec![TalosInputItem::Text {
                text: "hello".to_string(),
            }],
            stream: Some(false),
            workspace_id: None,
        };

        let result = process_session_message(
            state,
            test_auth(),
            HeaderMap::new(),
            "mock-session".to_string(),
            request,
        )
        .await
        .expect("message should be processed");

        assert_eq!(result.session_id, "mock-session");
        assert_eq!(result.stop_reason, "end_turn");
        assert_eq!(result.reasoning.len(), 1);
        assert!(matches!(
            result.output.first(),
            Some(TalosOutputItem::OutputText { text }) if text == "mock content"
        ));
    }

    #[tokio::test]
    async fn test_stream_session_message_uses_gateway_stream_path() {
        let state = Arc::new(HttpApiState::new(
            Arc::new(MockGateway),
            Arc::new(MockSessionService),
            "test-secret".to_string(),
            Some("test-token".to_string()),
            false,
            vec![test_model_info()],
        ));

        let request = TalosMessageRequest {
            input: vec![TalosInputItem::Text {
                text: "hello".to_string(),
            }],
            stream: Some(true),
            workspace_id: None,
        };

        let response =
            stream_session_message(state, test_auth(), "mock-session".to_string(), request)
                .await
                .expect("stream message should succeed");

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("stream body should be readable");
        let body_text = String::from_utf8_lossy(&body);
        assert!(body_text.contains("stream only content"));
    }

    #[tokio::test]
    async fn test_submit_tool_approval_handler_returns_no_content() {
        let state = Arc::new(HttpApiState::new(
            Arc::new(MockGateway),
            Arc::new(MockSessionService),
            "test-secret".to_string(),
            None,
            false,
            vec![test_model_info()],
        ));
        let app = Router::new()
            .route(
                "/v1/sessions/:session_id/tool-approvals",
                post(submit_tool_approval_handler),
            )
            .with_state(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/sessions/mock-session/tool-approvals")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "call_id": "call-1",
                            "tool_name": "exec_command",
                            "approved": false,
                            "reason": "deny"
                        })
                        .to_string(),
                    ))
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}
