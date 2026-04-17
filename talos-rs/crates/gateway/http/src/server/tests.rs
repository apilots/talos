use super::*;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::Value;
use talos_auth::{AuthResult, TokenType};
use talos_gateway::GatewayRequest;
use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Usage};
use talos_types::talos::{
    StopReason, TalosContext, TalosEvent, TalosEventPayload, TalosEventType, TalosInputItem,
    TalosMessageCompletedEvent, TalosMessageOutputDeltaEvent, TalosMessageRequest,
    TalosMessageStartedEvent, TalosMessageToolCallExecutingEvent,
    TalosMessageToolCallProgressDeltaEvent, TalosMessageToolCallQueuedEvent, TalosOutputItem,
    TalosProvider, TalosReasoning, TalosSourceType, TalosToolProgressKind, TalosToolStreamKey,
    TalosUsage,
};
use talos_types::SessionQuery;
use tower::util::ServiceExt;

use crate::server::messages::{process_session_message, stream_session_message};

struct MockGateway;
struct MockSessionService;
struct MockLlmProvider;

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

fn test_config() -> talos_types::TalosConfig {
    talos_types::TalosConfig::default()
}

fn isolated_test_config() -> (talos_types::TalosConfig, std::path::PathBuf) {
    let workspace_dir = std::env::temp_dir().join(format!("talos-http-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should exist");
    let mut config = test_config();
    config.agent.kernel.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    (config, workspace_dir)
}

fn test_state(auth_disabled: bool, static_token: Option<&str>) -> Arc<HttpApiState> {
    Arc::new(HttpApiState::new(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        test_config(),
        Arc::new(MockLlmProvider),
        "test-secret".to_string(),
        static_token.map(str::to_string),
        auth_disabled,
        vec![test_model_info()],
    ))
}

fn test_state_with_config(
    config: talos_types::TalosConfig,
    auth_disabled: bool,
    static_token: Option<&str>,
) -> Arc<HttpApiState> {
    Arc::new(HttpApiState::new(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        config,
        Arc::new(MockLlmProvider),
        "test-secret".to_string(),
        static_token.map(str::to_string),
        auth_disabled,
        vec![test_model_info()],
    ))
}

#[async_trait::async_trait]
impl LlmProvider for MockLlmProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        let content = if _request.messages.iter().any(|message| {
            message
                .content
                .contains("Respond ONLY with a valid JSON array of file paths.")
        }) {
            "[\"memory/2026-04-04.md\"]".to_string()
        } else if _request.messages.iter().any(|message| {
            message
                .content
                .contains("Respond ONLY with a valid JSON array.")
        }) {
            "[{\"path\":\"memory/2026-04-04.md\",\"from\":1,\"to\":5}]".to_string()
        } else {
            String::new()
        };
        Ok(ChatResponse {
            content,
            reasoning: Vec::new(),
            model: "mock-model".to_string(),
            usage: Usage::default(),
            finish_reason: None,
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        drop(tx);
        Ok(ChatStream { rx })
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

            let queued = TalosEvent {
                event_type: TalosEventType::MessageToolCallQueued,
                message_id: "msg_mock".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 2,
                payload: TalosEventPayload::MessageToolCallQueued(
                    TalosMessageToolCallQueuedEvent {
                        request_id: "req_mock".to_string(),
                        stream_key: TalosToolStreamKey { round: 0, index: 0 },
                        call_id: "call-1".to_string(),
                        tool_name: "exec_command".to_string(),
                        item_index: 1,
                        iteration: 2,
                        step_id: "step-2".to_string(),
                    },
                ),
            };
            let _ = tx.send(queued).await;

            let executing = TalosEvent {
                event_type: TalosEventType::MessageToolCallExecuting,
                message_id: "msg_mock".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 3,
                payload: TalosEventPayload::MessageToolCallExecuting(
                    TalosMessageToolCallExecutingEvent {
                        request_id: "req_mock".to_string(),
                        stream_key: TalosToolStreamKey { round: 0, index: 0 },
                        call_id: "call-1".to_string(),
                        tool_name: "exec_command".to_string(),
                        item_index: 2,
                        iteration: 3,
                        step_id: "step-3".to_string(),
                    },
                ),
            };
            let _ = tx.send(executing).await;

            let progress = TalosEvent {
                event_type: TalosEventType::MessageToolCallProgressDelta,
                message_id: "msg_mock".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 4,
                payload: TalosEventPayload::MessageToolCallProgressDelta(
                    TalosMessageToolCallProgressDeltaEvent {
                        request_id: "req_mock".to_string(),
                        stream_key: TalosToolStreamKey { round: 0, index: 0 },
                        call_id: "call-1".to_string(),
                        tool_name: "exec_command".to_string(),
                        message: "execution started".to_string(),
                        progress_kind: TalosToolProgressKind::Status,
                        item_index: 3,
                        iteration: 4,
                        step_id: "step-4".to_string(),
                    },
                ),
            };
            let _ = tx.send(progress).await;

            let completed = TalosEvent {
                event_type: TalosEventType::MessageCompleted,
                message_id: "msg_mock".to_string(),
                session_id: "mock-session".to_string(),
                sequence: 5,
                payload: TalosEventPayload::MessageCompleted(TalosMessageCompletedEvent {
                    request_id: "req_mock".to_string(),
                    stop_reason: StopReason::EndTurn,
                    usage: TalosUsage::default(),
                    total_output_items: 4,
                    iteration: 5,
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
        project_id: Option<String>,
        current_directory: Option<std::path::PathBuf>,
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
                project_id: project_id.clone(),
                current_directory: current_directory.clone(),
                project_root: context.project_root,
                environment: context.environment,
            },
            project_id,
            current_directory,
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
                project_id: None,
                current_directory: None,
                project_root: None,
                environment: None,
            },
            project_id: None,
            current_directory: None,
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

    async fn compact_session(&self, session_id: &str) -> TalosResult<talos_types::TalosSession> {
        self.get_session(session_id)
            .await?
            .ok_or_else(|| talos_error::TalosError::ResourceNotFound(session_id.to_string()))
    }

    async fn clear_session(&self, session_id: &str) -> TalosResult<talos_types::TalosSession> {
        let mut session = self
            .get_session(session_id)
            .await?
            .ok_or_else(|| talos_error::TalosError::ResourceNotFound(session_id.to_string()))?;
        session.state.message_count = 0;
        session.state.usage = talos_types::talos::TalosUsage::default();
        Ok(session)
    }

    async fn close_session(&self, _session_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn delete_session(
        &self,
        _session_id: &str,
    ) -> TalosResult<talos_types::SessionPatchAck> {
        Ok(talos_types::SessionPatchAck { success: true })
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

    async fn list_projects(&self) -> TalosResult<Vec<talos_types::ProjectRecord>> {
        Ok(Vec::new())
    }

    async fn get_project(
        &self,
        _project_id: &str,
    ) -> TalosResult<Option<talos_types::ProjectRecord>> {
        Ok(None)
    }

    async fn create_project(
        &self,
        name: String,
        directory: std::path::PathBuf,
    ) -> TalosResult<talos_types::ProjectRecord> {
        Ok(talos_types::ProjectRecord {
            project_id: "project-mock".to_string(),
            name,
            directory,
            created_at: 0,
            updated_at: 0,
            session_count: 0,
        })
    }

    async fn patch_project(
        &self,
        _project_id: &str,
        _patch: talos_types::ProjectPatch,
    ) -> TalosResult<Option<talos_types::ProjectRecord>> {
        Ok(None)
    }

    async fn delete_project(&self, _project_id: &str) -> TalosResult<()> {
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
    let state = test_state(false, None);
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
    let state = test_state(false, None);

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
    assert!(response.0["project_id"].is_null());
    assert!(response.0["current_directory"].is_null());
    assert_eq!(response.0["config"]["llm"]["model"], "qwen-plus");
}

#[tokio::test]
async fn test_list_sessions_handler_returns_data_array() {
    let state = test_state(false, None);

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
    let state = test_state(false, None);

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
    let state = test_state(false, None);

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
    let state = test_state(false, None);

    let response = close_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("close session should succeed");

    assert_eq!(response.0["success"], serde_json::Value::Bool(true));
}

#[tokio::test]
async fn test_delete_session_handler_returns_success() {
    let state = test_state(false, None);

    let response = delete_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("delete session should succeed");

    assert_eq!(response.0["success"], serde_json::Value::Bool(true));
}

#[tokio::test]
async fn test_compact_session_handler_returns_session_payload() {
    let state = test_state(false, None);

    let response = compact_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("compact session should succeed");

    assert_eq!(response.0["context"]["session_id"], "mock-session");
    assert_eq!(response.0["state"]["message_count"], 1);
}

#[tokio::test]
async fn test_clear_session_handler_returns_reset_session_payload() {
    let state = test_state(false, None);

    let response = clear_session_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("mock-session".to_string()),
    )
    .await
    .expect("clear session should succeed");

    assert_eq!(response.0["context"]["session_id"], "mock-session");
    assert_eq!(response.0["state"]["message_count"], 0);
    assert_eq!(response.0["state"]["usage"]["total_tokens"], 0);
}

#[tokio::test]
async fn test_get_workspace_doc_handler_returns_exists_false_for_missing_file() {
    let (config, workspace_dir) = isolated_test_config();
    let state = test_state_with_config(config, false, None);

    let response = get_workspace_doc_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Path("soul".to_string()),
    )
    .await
    .expect("get workspace doc should succeed");

    assert_eq!(response.0["kind"], "soul");
    assert_eq!(response.0["exists"], serde_json::Value::Bool(false));
    assert_eq!(response.0["content"], "");
    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[tokio::test]
async fn test_put_and_get_memory_file_handlers_roundtrip() {
    let (config, workspace_dir) = isolated_test_config();
    let state = test_state_with_config(config, false, None);

    let put_response = put_memory_file_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Path("2026-04-04".to_string()),
        Json(talos_types::DocUpdateRequest {
            content: "## [10:00] Example\nsession: session-1\n\n**Facts:**\n- stable fact\n".to_string(),
            base_version: talos_types::ExpectedFileVersion::MustNotExist,
        }),
    )
    .await
    .expect("put memory file should succeed");

    assert_eq!(put_response.0["date"], "2026-04-04");

    let get_response = get_memory_file_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Path("2026-04-04".to_string()),
    )
    .await
    .expect("get memory file should succeed");

    assert!(get_response.0["content"].as_str().unwrap().contains("stable fact"));

    let list_response = list_memory_files_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Query(serde_json::from_value(serde_json::json!({ "limit": 10 })).unwrap()),
    )
    .await
    .expect("list memory files should succeed");

    assert_eq!(list_response.0["data"][0]["date"], "2026-04-04");
    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[tokio::test]
async fn test_search_memory_files_handler_returns_matching_data() {
    let (config, workspace_dir) = isolated_test_config();
    let memory_dir = workspace_dir.join("memory");
    std::fs::create_dir_all(&memory_dir).expect("memory dir should exist");
    std::fs::write(
        memory_dir.join("2026-04-04.md"),
        "## [10:00] Example\nsession: session-1\n\n**Facts:**\n- stable fact\n",
    )
    .expect("memory file should write");
    let state = test_state_with_config(config, false, None);

    let response = search_memory_files_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Query(serde_json::from_value(serde_json::json!({
            "q": "stable",
            "limit": 5,
            "date_from": "2026-04-01",
            "date_to": "2026-04-30"
        }))
        .unwrap()),
    )
    .await
    .expect("search memory should succeed");

    assert_eq!(response.0["data"][0]["path"], "memory/2026-04-04.md");
    let _ = std::fs::remove_dir_all(workspace_dir);
}

#[test]
fn test_http_server_listen_addr_uses_configured_host() {
    let server = HttpServer::new(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        test_config(),
        Arc::new(MockLlmProvider),
        "127.0.0.1",
        8080,
        "qwen-plus",
        vec![test_model_info()],
    );

    assert_eq!(server.listen_addr(), "127.0.0.1:8080");
}

#[tokio::test]
async fn test_models_require_bearer_when_auth_enabled() {
    let state = test_state(false, None);
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
    let state = test_state(true, None);
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
        test_config(),
        Arc::new(MockLlmProvider),
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
        test_config(),
        Arc::new(MockLlmProvider),
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
    let state = test_state(false, Some("test-token"));

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
    let state = test_state(false, Some("test-token"));

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
    assert!(body_text.contains("event: message.tool_call.queued"));
    assert!(body_text.contains("event: message.tool_call.executing"));
    assert!(body_text.contains("event: message.tool_call.progress_delta"));
}

#[tokio::test]
async fn test_cancel_active_turn_handler_returns_success() {
    let state = test_state(false, Some("test-token"));

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
    let state = test_state(false, None);
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
