use super::*;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::Value;
use talos_auth::{AuthResult, TokenType};
use talos_gateway::GatewayRequest;
use talos_types::talos::{
    StopReason, TalosContext, TalosEvent, TalosEventPayload, TalosEventType, TalosInputItem,
    TalosMessageCompletedEvent, TalosMessageOutputDeltaEvent, TalosMessageRequest,
    TalosMessageStartedEvent, TalosOutputItem, TalosProvider, TalosReasoning, TalosSourceType,
    TalosUsage,
};
use talos_types::SessionQuery;
use tower::util::ServiceExt;

use crate::server::messages::{process_session_message, stream_session_message};

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
            stop_reason: StopReason::EndTurn,
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
                    stop_reason: StopReason::EndTurn,
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

    async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
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
            title: "Untitled Session".to_string(),
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
                environment: context.environment,
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
            title: "Untitled Session".to_string(),
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
                environment: None,
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

    async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
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

    async fn get_session_turns(
        &self,
        _session_id: &str,
    ) -> TalosResult<Vec<talos_types::talos::TalosMessageResponse>> {
        Ok(vec![])
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

fn test_context() -> TalosContext {
    TalosContext::new(
        TalosSourceType::Http,
        TalosProvider::Http,
        "static-token",
        "msg-1",
        "trace-1",
    )
    .with_channel_id("test-channel")
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

#[tokio::test]
async fn test_create_session_handler_returns_session_payload() {
    let state = Arc::new(HttpApiState::new(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        "test-secret".to_string(),
        None,
        false,
        vec![test_model_info()],
    ));

    let response = create_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Json(
            serde_json::from_value(serde_json::json!({
                "context": test_context(),
                "model": "qwen-plus"
            }))
            .expect("request should deserialize"),
        ),
    )
    .await
    .expect("create session should succeed");

    assert_eq!(response.0["context"]["session_id"], "mock-session");
    assert_eq!(response.0["config"]["llm"]["model"], "qwen-plus");
}

#[tokio::test]
async fn test_list_sessions_handler_returns_data_array() {
    let state = Arc::new(HttpApiState::new(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        "test-secret".to_string(),
        None,
        false,
        vec![test_model_info()],
    ));

    let response = list_sessions_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Query(SessionQuery::default()),
    )
    .await
    .expect("list sessions should succeed");

    assert!(response.0["data"].is_array());
}

#[tokio::test]
async fn test_get_session_handler_returns_not_found_for_unknown_session() {
    let state = Arc::new(HttpApiState::new(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        "test-secret".to_string(),
        None,
        false,
        vec![test_model_info()],
    ));

    let err = get_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("unknown-session".to_string()),
    )
    .await
    .expect_err("unknown session should return error");

    assert_eq!(err.into_response().status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_patch_session_handler_returns_success_ack() {
    let state = Arc::new(HttpApiState::new(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        "test-secret".to_string(),
        None,
        false,
        vec![test_model_info()],
    ));

    let response = patch_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
        Json(
            serde_json::from_value(serde_json::json!({
                "conf": {"agent.model": "qwen-plus"}
            }))
            .expect("patch request should deserialize"),
        ),
    )
    .await
    .expect("patch session should succeed");

    assert_eq!(response.0["success"], serde_json::Value::Bool(true));
}

#[tokio::test]
async fn test_close_session_handler_returns_success() {
    let state = Arc::new(HttpApiState::new(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        "test-secret".to_string(),
        None,
        false,
        vec![test_model_info()],
    ));

    let response = close_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("close session should succeed");

    assert_eq!(response.0["success"], serde_json::Value::Bool(true));
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
async fn test_lifecycle_gate_rejects_new_requests_when_quiescing() {
    let lifecycle = Arc::new(ServerLifecycle::new());
    lifecycle.store(ServerLifecycleState::Quiescing);
    let state = Arc::new(HttpApiState::new_with_lifecycle(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        "test-secret".to_string(),
        None,
        true,
        vec![test_model_info()],
        lifecycle,
    ));
    let app = Router::new()
        .route("/v1/models", get(list_models_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            lifecycle_gate_middleware,
        ))
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
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_lifecycle_gate_allows_health_when_quiescing() {
    let lifecycle = Arc::new(ServerLifecycle::new());
    lifecycle.store(ServerLifecycleState::Quiescing);
    let state = Arc::new(HttpApiState::new_with_lifecycle(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        "test-secret".to_string(),
        None,
        true,
        vec![test_model_info()],
        lifecycle,
    ));
    let app = Router::new()
        .route("/health", get(health_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            lifecycle_gate_middleware,
        ))
        .layer(middleware::from_fn_with_state(state, auth_middleware));

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
    assert_eq!(result.stop_reason, talos_types::talos::StopReason::EndTurn);
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

    let response = stream_session_message(state, test_auth(), "mock-session".to_string(), request)
        .await
        .expect("stream message should succeed");

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("stream body should be readable");
    let body_text = String::from_utf8_lossy(&body);
    assert!(body_text.contains("stream only content"));
}

#[tokio::test]
async fn test_cancel_active_turn_handler_returns_success() {
    let state = Arc::new(HttpApiState::new(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        "test-secret".to_string(),
        Some("test-token".to_string()),
        false,
        vec![test_model_info()],
    ));

    let result = cancel_active_turn_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("cancel should succeed");

    assert_eq!(result.0["success"], serde_json::Value::Bool(true));
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
