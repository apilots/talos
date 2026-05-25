mod access;
mod admin_users;
mod auth;
mod errors;
mod events;
mod lifecycle;
mod memory_files;
mod messages;
mod misc;
mod projects;
mod sessions;
mod state;
mod user_profile;
mod workspace_docs;

use std::path::PathBuf;
use std::sync::Arc;

use axum::middleware::{self};
use axum::routing::{get, post};
use axum::Router;
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tracing::{error, info};

use self::admin_users::{
    create_admin_user_handler, delete_admin_session_handler, delete_admin_user_handler,
    list_admin_projects_handler, list_admin_sessions_handler, list_admin_users_handler,
    patch_admin_user_handler,
};
use self::auth::{auth_middleware, lifecycle_gate_middleware};
use self::events::{
    get_active_turn_handler, recover_timeline_handler, subscribe_timeline_events_handler,
};
pub use self::lifecycle::{ServerLifecycle, ServerLifecycleState};
use self::memory_files::{
    get_memory_file_handler, list_memory_files_handler, put_memory_file_handler,
    search_memory_files_handler,
};
use self::messages::{
    retry_message_attempt_handler, session_message_handler, submit_dynamic_tool_result_handler,
};
use self::misc::{
    get_llm_handler, health_handler, list_models_handler, put_llm_handler, shutdown_signal,
};
use self::projects::{
    create_project_handler, delete_project_handler, get_project_handler, list_projects_handler,
    patch_project_handler,
};
use self::sessions::{
    cancel_active_turn_handler, clear_session_handler, close_session_handler,
    compact_session_handler, create_session_handler, delete_session_handler, get_session_handler,
    get_session_history_handler, list_sessions_handler, patch_session_handler,
};
use self::state::HttpApiState;
use self::user_profile::{
    create_user_token_handler, current_user_handler, delete_user_token_handler,
    list_user_tokens_handler, local_auth_removed_handler, logout_handler,
    patch_current_user_handler,
};
use self::workspace_docs::{
    get_workspace_doc_handler, list_workspace_docs_handler, put_workspace_doc_handler,
};
use talos_application_service::TalosApplicationService;
use talos_auth::AuthService;
use talos_error::TalosResult;
use talos_session_store::TimelineEventService;
use talos_types::TalosConfig;

/// HTTP Server for the Talos Gateway.
pub struct HttpServer {
    application_service: Arc<TalosApplicationService>,
    #[cfg(test)]
    event_service: Arc<TimelineEventService>,
    auth_service: Arc<dyn AuthService>,
    base_config: TalosConfig,
    host: String,
    port: u16,
    auth_disabled: bool,
    webui_dir: Option<PathBuf>,
    lifecycle: Arc<ServerLifecycle>,
}

impl HttpServer {
    /// Create a new HTTP server instance.
    pub fn new(
        application_service: Arc<TalosApplicationService>,
        event_service: Arc<TimelineEventService>,
        auth_service: Arc<dyn AuthService>,
        base_config: TalosConfig,
        host: impl Into<String>,
        port: u16,
    ) -> Self {
        #[cfg(not(test))]
        let _ = event_service;
        Self {
            application_service,
            #[cfg(test)]
            event_service,
            auth_service,
            base_config,
            host: host.into(),
            port,
            auth_disabled: false,
            webui_dir: None,
            lifecycle: Arc::new(ServerLifecycle::new()),
        }
    }

    /// Disable auth checks for local mock mode.
    pub fn with_auth_disabled(mut self, disabled: bool) -> Self {
        self.auth_disabled = disabled;
        self
    }

    /// Serve the compiled WebUI from the HTTP server root.
    pub fn with_webui_dir(mut self, webui_dir: impl Into<PathBuf>) -> Self {
        self.webui_dir = Some(webui_dir.into());
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
        let gc_shutdown = shutdown.clone();
        let app_events_for_gc = self.application_service.events.clone();
        let gc_task = tokio::spawn(async move {
            app_events_for_gc.run_event_log_gc_loop(gc_shutdown).await;
        });

        let state = Arc::new(HttpApiState::from_application_service(
            self.application_service.clone(),
            #[cfg(test)]
            self.event_service.clone(),
            self.base_config.clone(),
            self.auth_service.clone(),
            self.auth_disabled,
            self.lifecycle.clone(),
        ));

        let mut app = Router::new()
            .route("/health", get(health_handler))
            .route("/v1/auth/register", post(local_auth_removed_handler))
            .route("/v1/auth/login", post(local_auth_removed_handler))
            .route("/v1/models", get(list_models_handler))
            .route("/v1/auth/logout", post(logout_handler))
            .route(
                "/v1/user",
                get(current_user_handler).patch(patch_current_user_handler),
            )
            .route(
                "/v1/user/tokens",
                get(list_user_tokens_handler).post(create_user_token_handler),
            )
            .route(
                "/v1/user/tokens/:token_id",
                axum::routing::delete(delete_user_token_handler),
            )
            .route(
                "/v1/admin/users",
                get(list_admin_users_handler).post(create_admin_user_handler),
            )
            .route(
                "/v1/admin/users/:user_id",
                axum::routing::patch(patch_admin_user_handler).delete(delete_admin_user_handler),
            )
            .route("/v1/admin/sessions", get(list_admin_sessions_handler))
            .route(
                "/v1/admin/sessions/:session_id",
                axum::routing::delete(delete_admin_session_handler),
            )
            .route("/v1/admin/projects", get(list_admin_projects_handler))
            .route("/v1/llm", get(get_llm_handler).put(put_llm_handler))
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
                "/v1/sessions/:session_id/events",
                get(subscribe_timeline_events_handler),
            )
            .route(
                "/v1/sessions/:session_id/history",
                get(get_session_history_handler),
            )
            .route(
                "/v1/sessions/:session_id/recovery",
                get(recover_timeline_handler),
            )
            .route(
                "/v1/sessions/:session_id/active-turn",
                get(get_active_turn_handler),
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
                "/v1/sessions/:session_id/message-attempts",
                post(retry_message_attempt_handler),
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

        if let Some(webui_dir) = self.webui_dir.as_ref() {
            app = app.fallback_service(
                ServeDir::new(webui_dir).fallback(ServeFile::new(webui_dir.join("index.html"))),
            );
        }

        let listener = tokio::net::TcpListener::bind(self.listen_addr())
            .await
            .map_err(|e| {
                error!("Failed to bind to {}:{}: {}", self.host, self.port, e);
                talos_error::TalosError::NetworkError(e.to_string())
            })?;

        info!("HTTP server listening on {}:{}", self.host, self.port);

        let serve_result = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown.cancelled().await;
            })
            .await
            .map_err(|e| {
                error!("Server error: {}", e);
                talos_error::TalosError::NetworkError(e.to_string())
            });

        gc_task.abort();
        serve_result
    }

    fn listen_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod tests;
