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
use talos_types::canonical::{
    CanonicalContext, CanonicalEventType, CanonicalInputItem, CanonicalMessageRequest,
    CanonicalMessageResponse, CanonicalOutputItem,
};
use talos_types::{SessionPatch, SessionQuery, SessionService};

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
    model_provider: String,
    idempotency_cache: Arc<RwLock<HashMap<String, CanonicalMessageResponse>>>,
    models: Arc<Vec<String>>,
}

impl HttpApiState {
    fn new(
        gateway: Arc<dyn Gateway>,
        session_service: Arc<dyn SessionService>,
        jwt_secret: String,
        static_token: Option<String>,
        auth_disabled: bool,
        model_provider: String,
        models: Vec<String>,
    ) -> Self {
        Self {
            gateway,
            session_service,
            auth: Arc::new(JwtAuth::new(&jwt_secret)),
            static_token,
            auth_disabled,
            model_provider,
            idempotency_cache: Arc::new(RwLock::new(HashMap::new())),
            models: Arc::new(models),
        }
    }
}

/// HTTP Server for the Talos Gateway.
pub struct HttpServer {
    gateway: Arc<dyn Gateway>,
    session_service: Arc<dyn SessionService>,
    port: u16,
    jwt_secret: String,
    static_token: Option<String>,
    auth_disabled: bool,
    model_provider: String,
    models: Vec<String>,
}

impl HttpServer {
    /// Create a new HTTP server instance.
    pub fn new(
        gateway: Arc<dyn Gateway>,
        session_service: Arc<dyn SessionService>,
        port: u16,
        _default_model: impl Into<String>,
        model_provider: impl Into<String>,
        models: Vec<String>,
    ) -> Self {
        Self {
            gateway,
            session_service,
            port,
            jwt_secret: String::new(),
            static_token: None,
            auth_disabled: false,
            model_provider: model_provider.into(),
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
        info!("Starting HTTP server on port {}", self.port);

        let state = Arc::new(HttpApiState::new(
            self.gateway.clone(),
            self.session_service.clone(),
            self.jwt_secret.clone(),
            self.static_token.clone(),
            self.auth_disabled,
            self.model_provider.clone(),
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
                get(get_session_handler).patch(patch_session_handler),
            )
            .route(
                "/v1/sessions/:session_id/messages",
                post(session_message_handler),
            )
            .with_state(state.clone())
            .layer(middleware::from_fn_with_state(state, auth_middleware))
            .layer(CorsLayer::permissive());

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", self.port))
            .await
            .map_err(|e| {
                error!("Failed to bind to port {}: {}", self.port, e);
                talos_error::TalosError::NetworkError(e.to_string())
            })?;

        info!("HTTP server listening on port {}", self.port);

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .map_err(|e| {
                error!("Server error: {}", e);
                talos_error::TalosError::NetworkError(e.to_string())
            })?;

        Ok(())
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
    let data: Vec<Value> = state
        .models
        .iter()
        .map(|id| json!({ "id": id, "provider": state.model_provider }))
        .collect();
    Ok(Json(json!({ "data": data })))
}

#[derive(Debug, Deserialize)]
struct CreateSessionRequest {
    context: CanonicalContext,
    model: Option<String>,
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct PatchSessionRequest {
    model: Option<String>,
    metadata: Option<Value>,
}

async fn create_session_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<AuthResult>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<Json<Value>, ApiError> {
    validate_auth_context(&auth.0, &request.context, DEFAULT_SCOPE_SESSIONS_WRITE)?;
    if let Some(model) = request.model.as_deref() {
        validate_model_supported(&state, model)?;
    }

    let session = state
        .session_service
        .create_session(request.context, request.model, request.metadata)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                e.to_string(),
            )
        })?;

    Ok(Json(json!({
        "id": session.id,
        "model": session.model,
        "created_at": session.created_at,
        "context": session.context,
    })))
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
    let data: Vec<Value> = sessions
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "model": s.model,
                "message_count": s.message_count,
                "last_activity": s.last_activity,
                "context": s.context,
            })
        })
        .collect();

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

    Ok(Json(json!({
        "id": session.id,
        "model": session.model,
        "message_count": session.message_count,
        "last_activity": session.last_activity,
        "context": session.context,
        "metadata": session.metadata,
    })))
}

async fn patch_session_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<AuthResult>,
    Path(session_id): Path<String>,
    Json(request): Json<PatchSessionRequest>,
) -> Result<Json<Value>, ApiError> {
    if let Some(model) = request.model.as_deref() {
        validate_model_supported(&state, model)?;
    }

    let patch = SessionPatch {
        model: request.model,
        metadata: request.metadata,
    };
    let Some(session) = state
        .session_service
        .patch_session(&session_id, patch)
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

    Ok(Json(json!({
        "id": session.id,
        "model": session.model,
        "message_count": session.message_count,
        "last_activity": session.last_activity,
        "context": session.context,
        "metadata": session.metadata,
    })))
}

async fn session_message_handler(
    State(state): State<Arc<HttpApiState>>,
    auth: axum::extract::Extension<AuthResult>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(request): Json<CanonicalMessageRequest>,
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
    request: CanonicalMessageRequest,
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

    validate_auth_context(&auth, &session.context, DEFAULT_SCOPE_MESSAGES_WRITE)?;
    if let Some(model) = request.model.as_deref() {
        validate_model_supported(&state, model)?;
    }

    let input_text = extract_input_text(&request.input)?;
    let context = session.context.clone();
    let gateway_request = GatewayRequest {
        channel: format!("{}:{}", context.provider, context.channel_id),
        user_id: context.user_id.clone(),
        content: input_text,
        model: request.model.clone().or(Some(session.model.clone())),
        session_id: Some(session_id.clone()),
        context: Some(context),
        metadata: request.metadata.clone(),
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
        let event_name = canonical_event_name(&event.event_type);
        let data = serde_json::to_string(&event).expect("failed to serialize canonical event");
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
    request: CanonicalMessageRequest,
) -> Result<CanonicalMessageResponse, ApiError> {
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

    validate_auth_context(&auth, &session.context, DEFAULT_SCOPE_MESSAGES_WRITE)?;
    if let Some(model) = request.model.as_deref() {
        validate_model_supported(&state, model)?;
    }

    let idempotency_key = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|v| format!("{}:{}", session_id, v));

    if let Some(key) = &idempotency_key {
        if let Some(cached) = state.idempotency_cache.read().await.get(key) {
            return Ok(cached.clone());
        }
    }

    let input_text = extract_input_text(&request.input)?;
    let context = session.context.clone();

    let gateway_request = GatewayRequest {
        channel: format!("{}:{}", context.provider, context.channel_id),
        user_id: context.user_id.clone(),
        content: input_text,
        model: request.model.clone().or(Some(session.model.clone())),
        session_id: Some(session_id.clone()),
        context: Some(context),
        metadata: request.metadata.clone(),
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
        vec![CanonicalOutputItem::OutputText {
            text: gateway_response.content.clone(),
        }]
    } else {
        gateway_response.output_items.clone()
    };

    let response = CanonicalMessageResponse {
        request_id: format!("req_{}", Uuid::new_v4().simple()),
        message_id: format!("msg_{}", Uuid::new_v4().simple()),
        session_id,
        model: gateway_response.model.clone(),
        output,
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

fn extract_input_text(input: &[CanonicalInputItem]) -> Result<String, ApiError> {
    let mut texts = Vec::new();
    for item in input {
        if let CanonicalInputItem::InputText { text } = item {
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
            "input must include at least one non-empty input_text item",
        ));
    }

    Ok(texts.join("\n"))
}

fn validate_model_supported(state: &HttpApiState, model: &str) -> Result<(), ApiError> {
    if state.models.iter().any(|v| v == model) {
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
    context: &CanonicalContext,
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
                    .any(|v| v == "*" || v == &context.channel_id)
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

fn canonical_event_name(event_type: &CanonicalEventType) -> &'static str {
    match event_type {
        CanonicalEventType::MessageStarted => "message.started",
        CanonicalEventType::MessageReasoningDelta => "message.reasoning.delta",
        CanonicalEventType::MessageOutputDelta => "message.output.delta",
        CanonicalEventType::MessageToolCallStarted => "message.tool_call.started",
        CanonicalEventType::MessageToolCallCompleted => "message.tool_call.completed",
        CanonicalEventType::MessageCompleted => "message.completed",
        CanonicalEventType::MessageError => "message.error",
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
    use talos_types::canonical::CanonicalEvent;
    use tower::util::ServiceExt;

    struct MockGateway;
    struct MockSessionService;

    #[async_trait::async_trait]
    impl Gateway for MockGateway {
        async fn handle_request(
            &self,
            _request: GatewayRequest,
        ) -> TalosResult<talos_gateway::GatewayResponse> {
            Ok(talos_gateway::GatewayResponse {
                content: "mock content".to_string(),
                output_items: vec![CanonicalOutputItem::OutputText {
                    text: "mock content".to_string(),
                }],
                stop_reason: "end_turn".to_string(),
                usage: talos_types::canonical::CanonicalUsage::default(),
                success: true,
                model: "qwen-max".to_string(),
                session_id: Some("mock-session".to_string()),
            })
        }

        async fn handle_request_stream(
            &self,
            _request: GatewayRequest,
        ) -> TalosResult<tokio::sync::mpsc::Receiver<CanonicalEvent>> {
            let (tx, rx) = tokio::sync::mpsc::channel(8);
            tokio::spawn(async move {
                let _ = tx
                    .send(CanonicalEvent {
                        event_type: CanonicalEventType::MessageStarted,
                        message_id: "msg_mock".to_string(),
                        session_id: "mock-session".to_string(),
                        sequence: 0,
                        data: json!({
                            "request_id": "req_mock",
                            "model": "qwen-max",
                            "usage": talos_types::canonical::CanonicalUsage::default(),
                            "iteration": 0_u64,
                            "step_id": "start",
                        }),
                    })
                    .await;
                let _ = tx
                    .send(CanonicalEvent {
                        event_type: CanonicalEventType::MessageOutputDelta,
                        message_id: "msg_mock".to_string(),
                        session_id: "mock-session".to_string(),
                        sequence: 1,
                        data: json!({
                            "request_id": "req_mock",
                            "delta": "stream only content",
                            "item_index": 0_u64,
                            "iteration": 1_u64,
                            "step_id": "step-1",
                        }),
                    })
                    .await;
                let _ = tx
                    .send(CanonicalEvent {
                        event_type: CanonicalEventType::MessageCompleted,
                        message_id: "msg_mock".to_string(),
                        session_id: "mock-session".to_string(),
                        sequence: 2,
                        data: json!({
                            "request_id": "req_mock",
                            "stop_reason": "end_turn",
                            "usage": talos_types::canonical::CanonicalUsage::default(),
                            "total_output_items": 1_u64,
                            "iteration": 2_u64,
                            "step_id": "completed",
                        }),
                    })
                    .await;
            });
            Ok(rx)
        }

        async fn start(&self) -> TalosResult<()> {
            Ok(())
        }
    }

    fn mock_context() -> CanonicalContext {
        CanonicalContext {
            source_type: "api".to_string(),
            provider: "test".to_string(),
            channel_id: "test-channel".to_string(),
            user_id: "static-token".to_string(),
            thread_id: None,
            message_id: None,
            trace_id: None,
        }
    }

    #[async_trait::async_trait]
    impl SessionService for MockSessionService {
        async fn create_session(
            &self,
            context: CanonicalContext,
            model: Option<String>,
            metadata: Option<Value>,
        ) -> TalosResult<talos_types::SessionRecord> {
            Ok(talos_types::SessionRecord {
                id: "mock-session".to_string(),
                context,
                model: model.unwrap_or_else(|| "qwen-max".to_string()),
                created_at: 0,
                last_activity: 0,
                message_count: 0,
                metadata,
            })
        }

        async fn list_sessions(
            &self,
            _query: talos_types::SessionQuery,
        ) -> TalosResult<Vec<talos_types::SessionRecord>> {
            Ok(Vec::new())
        }

        async fn get_session(
            &self,
            session_id: &str,
        ) -> TalosResult<Option<talos_types::SessionRecord>> {
            if session_id != "mock-session" {
                return Ok(None);
            }
            Ok(Some(talos_types::SessionRecord {
                id: "mock-session".to_string(),
                context: mock_context(),
                model: "qwen-max".to_string(),
                created_at: 0,
                last_activity: 0,
                message_count: 1,
                metadata: None,
            }))
        }

        async fn patch_session(
            &self,
            _session_id: &str,
            _patch: talos_types::SessionPatch,
        ) -> TalosResult<Option<talos_types::SessionRecord>> {
            Ok(None)
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
            "bailian".to_string(),
            vec!["qwen-max".to_string()],
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
    }

    #[tokio::test]
    async fn test_models_require_bearer_when_auth_enabled() {
        let state = Arc::new(HttpApiState::new(
            Arc::new(MockGateway),
            Arc::new(MockSessionService),
            "test-secret".to_string(),
            None,
            false,
            "bailian".to_string(),
            vec!["qwen-max".to_string()],
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
            "bailian".to_string(),
            vec!["qwen-max".to_string()],
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
            "bailian".to_string(),
            vec!["qwen-max".to_string()],
        ));

        let request = CanonicalMessageRequest {
            input: vec![CanonicalInputItem::InputText {
                text: "hello".to_string(),
            }],
            stream: Some(false),
            model: None,
            metadata: None,
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
        assert!(matches!(
            result.output.first(),
            Some(CanonicalOutputItem::OutputText { text }) if text == "mock content"
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
            "bailian".to_string(),
            vec!["qwen-max".to_string()],
        ));

        let request = CanonicalMessageRequest {
            input: vec![CanonicalInputItem::InputText {
                text: "hello".to_string(),
            }],
            stream: Some(true),
            model: None,
            metadata: None,
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
}
