use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde_json::{json, Value};

use talos_auth::RequestIdentity;
use talos_error::TalosError;
use talos_types::{DocUpdateRequest, WorkspaceDocDetail, WorkspaceDocKind, WorkspaceDocSummary};

use super::errors::ApiError;
use super::file_support::{
    content_hash, modified_at_ms, read_optional, validate_expected_version, with_file_lock,
    workspace_root, write_atomic,
};
use super::state::HttpApiState;

fn workspace_doc_path(state: &HttpApiState, kind: WorkspaceDocKind) -> PathBuf {
    workspace_root(&state.base_config).join(kind.file_name())
}

fn parse_doc_kind(raw: &str) -> Result<WorkspaceDocKind, ApiError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "soul" => Ok(WorkspaceDocKind::Soul),
        "identity" => Ok(WorkspaceDocKind::Identity),
        "user" => Ok(WorkspaceDocKind::User),
        "heartbeat" => Ok(WorkspaceDocKind::Heartbeat),
        "memory" => Ok(WorkspaceDocKind::Memory),
        _ => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "unsupported workspace doc kind",
        )),
    }
}

async fn load_detail(
    state: &HttpApiState,
    kind: WorkspaceDocKind,
) -> Result<WorkspaceDocDetail, ApiError> {
    let path = workspace_doc_path(state, kind);
    let content = read_optional(&path).await.map_err(map_internal_error)?;
    let updated_at = modified_at_ms(&path).await.map_err(map_internal_error)?;
    let detail = match content {
        Some(content) => WorkspaceDocDetail {
            kind,
            path: kind.file_name().to_string(),
            exists: true,
            updated_at,
            version_token: Some(content_hash(&content)),
            content,
        },
        None => WorkspaceDocDetail {
            kind,
            path: kind.file_name().to_string(),
            exists: false,
            updated_at: None,
            version_token: None,
            content: String::new(),
        },
    };
    Ok(detail)
}

fn map_internal_error(err: TalosError) -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "internal_error",
        err.to_string(),
    )
}

fn map_write_error(err: TalosError) -> ApiError {
    match err {
        TalosError::ValidationError(message) => {
            ApiError::new(StatusCode::CONFLICT, "invalid_request", message)
        }
        other => map_internal_error(other),
    }
}

pub(super) async fn list_workspace_docs_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<RequestIdentity>,
) -> Result<Json<Value>, ApiError> {
    let mut docs = Vec::new();
    for kind in [
        WorkspaceDocKind::Soul,
        WorkspaceDocKind::Identity,
        WorkspaceDocKind::User,
        WorkspaceDocKind::Heartbeat,
        WorkspaceDocKind::Memory,
    ] {
        let detail = load_detail(state.as_ref(), kind).await?;
        docs.push(WorkspaceDocSummary {
            kind,
            path: detail.path,
            exists: detail.exists,
            updated_at: detail.updated_at,
            version_token: detail.version_token,
        });
    }
    Ok(Json(json!({ "data": docs })))
}

pub(super) async fn get_workspace_doc_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<RequestIdentity>,
    Path(kind): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let kind = parse_doc_kind(&kind)?;
    Ok(Json(json!(load_detail(state.as_ref(), kind).await?)))
}

pub(super) async fn put_workspace_doc_handler(
    State(state): State<Arc<HttpApiState>>,
    _auth: axum::extract::Extension<RequestIdentity>,
    Path(kind): Path<String>,
    Json(request): Json<DocUpdateRequest>,
) -> Result<Json<Value>, ApiError> {
    let kind = parse_doc_kind(&kind)?;
    let path = workspace_doc_path(state.as_ref(), kind);
    with_file_lock(state.as_ref(), &path, async {
        let existing = read_optional(&path).await?;
        validate_expected_version(existing.as_deref(), &request)?;
        write_atomic(&path, &request.content).await?;
        Ok(())
    })
    .await
    .map_err(map_write_error)?;

    Ok(Json(json!(load_detail(state.as_ref(), kind).await?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use axum::response::IntoResponse;
    use talos_auth::{
        AuthManager, AuthService, InMemoryUserStore, JwtAuth, RequestIdentity, UserService,
    };
    use talos_error::{TalosError, TalosResult};
    use talos_gateway::{Gateway, GatewayRequest, GatewayResponse};
    use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Usage};
    use talos_types::talos::{TalosContext, TalosEvent, TalosMessageResponse};
    use talos_types::{
        ModelCompatConfig, ModelCostConfig, ModelInfo, ProjectPatch, ProjectRecord, SessionPatch,
        SessionPatchAck, SessionQuery, SessionService, TalosConfig, TalosSession,
    };

    use crate::server::state::HttpApiState;

    struct NoopGateway;
    struct NoopSessionService;
    struct NoopLlmProvider;

    fn test_auth() -> axum::extract::Extension<RequestIdentity> {
        axum::extract::Extension(RequestIdentity::mock_admin("workspace-doc-test"))
    }

    fn test_auth_services() -> (Arc<dyn AuthService>, Arc<dyn UserService>) {
        let manager = Arc::new(AuthManager::new(
            Arc::new(InMemoryUserStore::default()),
            Arc::new(JwtAuth::new("test-secret")),
            true,
        ));
        let auth_service: Arc<dyn AuthService> = manager.clone();
        let user_service: Arc<dyn UserService> = manager;
        (auth_service, user_service)
    }

    fn test_model_info() -> ModelInfo {
        ModelInfo {
            id: "qwen-plus".to_string(),
            name: "Qwen Plus".to_string(),
            provider: "bailian".to_string(),
            context_window: 32_768,
            max_tokens: 8_192,
            reasoning: true,
            input: vec![talos_types::ModelInputKind::Text],
            cost: ModelCostConfig::default(),
            compat: ModelCompatConfig::default(),
        }
    }

    fn test_state() -> (Arc<HttpApiState>, PathBuf) {
        let workspace_dir =
            std::env::temp_dir().join(format!("talos-workspace-doc-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace_dir).expect("workspace dir should exist");
        let mut config = TalosConfig::default();
        config.agent.kernel.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
        let (auth_service, user_service) = test_auth_services();
        let state = Arc::new(HttpApiState::new(
            Arc::new(NoopGateway),
            Arc::new(NoopSessionService),
            config,
            Arc::new(NoopLlmProvider),
            auth_service,
            user_service,
            "test-secret".to_string(),
            None,
            true,
            vec![test_model_info()],
        ));
        (state, workspace_dir)
    }

    #[async_trait::async_trait]
    impl Gateway for NoopGateway {
        async fn handle_request(&self, _request: GatewayRequest) -> TalosResult<GatewayResponse> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn handle_request_stream(
            &self,
            _request: GatewayRequest,
        ) -> TalosResult<tokio::sync::mpsc::Receiver<TalosEvent>> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
            Ok(())
        }

        async fn start(&self) -> TalosResult<()> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl LlmProvider for NoopLlmProvider {
        fn name(&self) -> &str {
            "noop"
        }

        fn default_model(&self) -> &str {
            "noop-model"
        }

        async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
            Ok(ChatResponse {
                content: String::new(),
                reasoning: Vec::new(),
                model: "noop-model".to_string(),
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
    impl SessionService for NoopSessionService {
        async fn create_session(
            &self,
            _context: TalosContext,
            _model: Option<String>,
            _conf: Option<talos_types::SessionConf>,
            _project_id: Option<String>,
            _current_directory: Option<PathBuf>,
        ) -> TalosResult<TalosSession> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn list_sessions(&self, _query: SessionQuery) -> TalosResult<Vec<TalosSession>> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn get_session(&self, _session_id: &str) -> TalosResult<Option<TalosSession>> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn patch_session(
            &self,
            _session_id: &str,
            _patch: SessionPatch,
        ) -> TalosResult<SessionPatchAck> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn compact_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn clear_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn close_session(&self, _session_id: &str) -> TalosResult<()> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn delete_session(&self, _session_id: &str) -> TalosResult<SessionPatchAck> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn submit_tool_approval(
            &self,
            _session_id: &str,
            _call_id: &str,
            _tool_name: &str,
            _approved: bool,
            _reason: Option<String>,
        ) -> TalosResult<()> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn submit_dynamic_tool_result(
            &self,
            _session_id: &str,
            _call_id: &str,
            _tool_name: &str,
            _output: String,
            _success: bool,
        ) -> TalosResult<()> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn get_session_turns(
            &self,
            _session_id: &str,
        ) -> TalosResult<Vec<TalosMessageResponse>> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn get_project(&self, _project_id: &str) -> TalosResult<Option<ProjectRecord>> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn create_project(
            &self,
            _name: String,
            _directory: PathBuf,
            _owner_user_id: String,
        ) -> TalosResult<ProjectRecord> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn patch_project(
            &self,
            _project_id: &str,
            _patch: ProjectPatch,
        ) -> TalosResult<Option<ProjectRecord>> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }

        async fn delete_project(&self, _project_id: &str) -> TalosResult<()> {
            Err(TalosError::ToolError(
                "not used in workspace docs tests".to_string(),
            ))
        }
    }

    #[tokio::test]
    async fn test_put_workspace_doc_handler_creates_missing_doc() {
        let (state, workspace_dir) = test_state();

        let response = put_workspace_doc_handler(
            State(state.clone()),
            test_auth(),
            Path("soul".to_string()),
            Json(DocUpdateRequest {
                content: "soul content".to_string(),
                base_version: talos_types::ExpectedFileVersion::MustNotExist,
            }),
        )
        .await
        .expect("put workspace doc should succeed");

        assert_eq!(response.0["exists"], true);
        assert_eq!(response.0["content"], "soul content");
        assert_eq!(
            std::fs::read_to_string(workspace_dir.join("SOUL.md"))
                .expect("workspace doc should exist on disk"),
            "soul content"
        );
        let _ = std::fs::remove_dir_all(workspace_dir);
    }

    #[tokio::test]
    async fn test_put_workspace_doc_handler_rejects_stale_version() {
        let (state, workspace_dir) = test_state();
        std::fs::write(workspace_dir.join("IDENTITY.md"), "current version")
            .expect("seed doc should write");

        let error = put_workspace_doc_handler(
            State(state),
            test_auth(),
            Path("identity".to_string()),
            Json(DocUpdateRequest {
                content: "updated".to_string(),
                base_version: talos_types::ExpectedFileVersion::Match {
                    version_token: "stale".to_string(),
                },
            }),
        )
        .await
        .expect_err("stale version should be rejected");

        assert_eq!(error.into_response().status(), StatusCode::CONFLICT);
        let _ = std::fs::remove_dir_all(workspace_dir);
    }

    #[tokio::test]
    async fn test_list_workspace_docs_handler_returns_supported_kinds() {
        let (state, workspace_dir) = test_state();

        let response = list_workspace_docs_handler(State(state), test_auth())
            .await
            .expect("list workspace docs should succeed");

        let docs = response.0["data"]
            .as_array()
            .expect("data should be an array");
        assert_eq!(docs.len(), 5);
        assert_eq!(docs[0]["kind"], "soul");
        assert_eq!(docs[4]["kind"], "memory");
        let _ = std::fs::remove_dir_all(workspace_dir);
    }
}
