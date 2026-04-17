mod auth;
mod errors;
mod file_support;
mod lifecycle;
mod messages;
mod memory_files;
mod misc;
mod projects;
mod sessions;
mod state;
mod workspace_docs;

use std::sync::Arc;

use axum::middleware::{self};
use axum::routing::{get, post};
use axum::Router;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

use self::auth::{auth_middleware, lifecycle_gate_middleware};
pub use self::lifecycle::{ServerLifecycle, ServerLifecycleState};
use self::messages::{
    session_message_handler, submit_dynamic_tool_result_handler, submit_tool_approval_handler,
};
use self::memory_files::{
    get_memory_file_handler, list_memory_files_handler, put_memory_file_handler,
    search_memory_files_handler,
};
use self::misc::{health_handler, list_models_handler, shutdown_signal};
use self::projects::{
    create_project_handler, delete_project_handler, get_project_handler, list_projects_handler,
    patch_project_handler,
};
use self::sessions::{
    cancel_active_turn_handler, clear_session_handler, close_session_handler,
    compact_session_handler, create_session_handler, delete_session_handler, get_session_handler,
    get_session_turns_handler, list_sessions_handler, patch_session_handler,
};
use self::state::HttpApiState;
use self::workspace_docs::{
    get_workspace_doc_handler, list_workspace_docs_handler, put_workspace_doc_handler,
};
use talos_error::TalosResult;
use talos_gateway::Gateway;
use talos_llm_provider::LlmProvider;
use talos_types::{ModelInfo, SessionService, TalosConfig};

const DEFAULT_SCOPE_MESSAGES_WRITE: &str = "messages:write";
const DEFAULT_SCOPE_SESSIONS_WRITE: &str = "sessions:write";

/// HTTP Server for the Talos Gateway.
pub struct HttpServer {
    gateway: Arc<dyn Gateway>,
    session_service: Arc<dyn SessionService>,
    base_config: TalosConfig,
    llm_provider: Arc<dyn LlmProvider>,
    host: String,
    port: u16,
    jwt_secret: String,
    static_token: Option<String>,
    auth_disabled: bool,
    models: Vec<ModelInfo>,
    lifecycle: Arc<ServerLifecycle>,
}

impl HttpServer {
    /// Create a new HTTP server instance.
    pub fn new(
        gateway: Arc<dyn Gateway>,
        session_service: Arc<dyn SessionService>,
        base_config: TalosConfig,
        llm_provider: Arc<dyn LlmProvider>,
        host: impl Into<String>,
        port: u16,
        _default_model: impl Into<String>,
        models: Vec<ModelInfo>,
    ) -> Self {
        Self {
            gateway,
            session_service,
            base_config,
            llm_provider,
            host: host.into(),
            port,
            jwt_secret: String::new(),
            static_token: None,
            auth_disabled: false,
            models,
            lifecycle: Arc::new(ServerLifecycle::new()),
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

    pub fn lifecycle(&self) -> Arc<ServerLifecycle> {
        self.lifecycle.clone()
    }

    /// Start the HTTP server.
    pub async fn run(&self) -> TalosResult<()> {
        let shutdown = CancellationToken::new();
        let signal_shutdown = shutdown.clone();
        let signal_task = tokio::spawn(async move {
            shutdown_signal().await;
            signal_shutdown.cancel();
        });
        let result = self.run_with_shutdown(shutdown).await;
        signal_task.abort();
        result
    }

    pub async fn run_with_shutdown(&self, shutdown: CancellationToken) -> TalosResult<()> {
        info!("Starting HTTP server on {}:{}", self.host, self.port);

        let state = Arc::new(HttpApiState::new_with_lifecycle(
            self.gateway.clone(),
            self.session_service.clone(),
            self.base_config.clone(),
            self.llm_provider.clone(),
            self.jwt_secret.clone(),
            self.static_token.clone(),
            self.auth_disabled,
            self.models.clone(),
            self.lifecycle.clone(),
        ));

        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/v1/models", get(list_models_handler))
            .route(
                "/v1/sessions",
                get(list_sessions_handler).post(create_session_handler),
            )
            .route(
                "/v1/projects",
                get(list_projects_handler).post(create_project_handler),
            )
            .route(
                "/v1/projects/:project_id",
                get(get_project_handler)
                    .patch(patch_project_handler)
                    .delete(delete_project_handler),
            )
            .route("/v1/memory/files", get(list_memory_files_handler))
            .route(
                "/v1/memory/files/:date",
                get(get_memory_file_handler).put(put_memory_file_handler),
            )
            .route("/v1/memory/search", get(search_memory_files_handler))
            .route("/v1/workspace/docs", get(list_workspace_docs_handler))
            .route(
                "/v1/workspace/docs/:kind",
                get(get_workspace_doc_handler).put(put_workspace_doc_handler),
            )
            .route(
                "/v1/sessions/:session_id",
                get(get_session_handler)
                    .patch(patch_session_handler)
                    .delete(delete_session_handler),
            )
            .route(
                "/v1/sessions/:session_id/turns",
                get(get_session_turns_handler),
            )
            .route(
                "/v1/sessions/:session_id/close",
                post(close_session_handler),
            )
            .route(
                "/v1/sessions/:session_id/cancel",
                post(cancel_active_turn_handler),
            )
            .route(
                "/v1/sessions/:session_id/compact",
                post(compact_session_handler),
            )
            .route(
                "/v1/sessions/:session_id/clear",
                post(clear_session_handler),
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
            .layer(middleware::from_fn_with_state(
                state.clone(),
                lifecycle_gate_middleware,
            ))
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
            .with_graceful_shutdown(async move {
                shutdown.cancelled().await;
            })
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

#[cfg(test)]
mod tests;
