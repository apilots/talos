use super::*;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::Value;
use talos_application_service::TalosApplicationService;
use talos_auth::{
    AuthCacheConfig, AuthManager, AuthService, CreateUserTokenRequest, InMemoryUserStore,
    InMemoryUserTokenStore, JwtAuth, RequestIdentity, UserService,
};
use talos_config::{InMemoryLlmConfigStore, LlmConfigManager, LlmConfigService};
use talos_gateway::Gateway;
use talos_gateway::GatewayRequest;
use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Usage};
use talos_session_store::{
    ActiveTurnRecord, ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, IdempotencyRecord,
    IdempotencyReservation, IdempotencyStatus, MessageCommandAck, MessageIdempotencyStore,
    NewSessionEvent, PendingDynamicToolResult, PendingToolApproval, PendingUserAction,
    SessionEventId, SessionEventRecord, SessionEventRetentionConfig, SessionEventService,
    SessionEventStore, SessionStore,
};
use talos_types::talos::{
    StopReason, TalosContext, TalosEvent, TalosEventPayload, TalosEventType, TalosInputItem,
    TalosMessageCompletedEvent, TalosMessageOutputDeltaEvent, TalosMessageRequest,
    TalosMessageStartedEvent, TalosMessageToolCallExecutingEvent,
    TalosMessageToolCallProgressDeltaEvent, TalosMessageToolCallQueuedEvent, TalosOutputItem,
    TalosProvider, TalosReasoning, TalosSourceType, TalosToolProgressKind, TalosToolStreamKey,
    TalosUsage,
};
use talos_types::{
    ModelInfo, ProjectPatch, ProjectRecord, SessionConf, SessionPatch, SessionPatchAck,
    SessionQuery, SessionRecord, SessionRuntime, SessionService, TalosSession,
};
use tower::util::ServiceExt;

use crate::server::admin_users::{delete_admin_session_handler, list_admin_sessions_handler};
use crate::server::events::{get_active_turn_handler, subscribe_session_events_handler};
use crate::server::messages::{
    retry_message_attempt_handler, session_message_handler, submit_message_command,
};
use crate::server::misc::get_llm_handler;
use crate::server::user_profile::{
    create_user_token_handler, list_admin_users_handler, list_user_tokens_handler,
};

#[path = "tests/support.rs"]
mod support;
use support::*;

fn test_model_info() -> ModelInfo {
    ModelInfo {
        id: "qwen-plus".to_string(),
        name: "Qwen Plus".to_string(),
        provider: "bailian".to_string(),
        context_window: 32_768,
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
    let workspace_dir =
        std::env::temp_dir().join(format!("talos-http-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should exist");
    let mut config = test_config();
    config.agent.kernel.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    (config, workspace_dir)
}

fn test_state_with_lifecycle(
    auth_disabled: bool,
    lifecycle: Arc<ServerLifecycle>,
) -> Arc<HttpApiState> {
    let (auth_service, user_service) = test_auth_services();
    let event_store = Arc::new(RecordingEventStore::default());
    build_test_state_with_lifecycle(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        event_service_from_store(event_store.clone()),
        event_store,
        test_config(),
        Arc::new(MockLlmProvider),
        auth_service,
        user_service,
        auth_disabled,
        lifecycle,
    )
}

fn build_test_state_with_lifecycle(
    gateway: Arc<dyn Gateway>,
    session_service: Arc<dyn SessionService>,
    event_service: Arc<SessionEventService>,
    idempotency_store: Arc<dyn MessageIdempotencyStore>,
    base_config: talos_types::TalosConfig,
    llm_provider: Arc<dyn LlmProvider>,
    auth_service: Arc<dyn AuthService>,
    user_service: Arc<dyn UserService>,
    auth_disabled: bool,
    lifecycle: Arc<ServerLifecycle>,
) -> Arc<HttpApiState> {
    let llm_config_service: Arc<dyn LlmConfigService> = Arc::new(LlmConfigManager::new(Arc::new(
        InMemoryLlmConfigStore::new(base_config.llm.clone()),
    )));
    build_test_state_with_lifecycle_and_config(
        gateway,
        session_service,
        event_service,
        idempotency_store,
        base_config,
        llm_provider,
        auth_service,
        user_service,
        auth_disabled,
        vec![test_model_info()],
        llm_config_service,
        lifecycle,
    )
}

fn build_test_state_with_lifecycle_and_config(
    gateway: Arc<dyn Gateway>,
    session_service: Arc<dyn SessionService>,
    event_service: Arc<SessionEventService>,
    idempotency_store: Arc<dyn MessageIdempotencyStore>,
    base_config: talos_types::TalosConfig,
    llm_provider: Arc<dyn LlmProvider>,
    auth_service: Arc<dyn AuthService>,
    user_service: Arc<dyn UserService>,
    auth_disabled: bool,
    models: Vec<ModelInfo>,
    llm_config_service: Arc<dyn LlmConfigService>,
    lifecycle: Arc<ServerLifecycle>,
) -> Arc<HttpApiState> {
    let app = Arc::new(TalosApplicationService::new_with_session_store(
        gateway,
        Arc::new(LegacySessionServiceRuntime::new(session_service.clone())),
        Arc::new(LegacySessionStore::new(session_service)),
        event_service.clone(),
        idempotency_store,
        base_config.clone(),
        llm_provider,
        llm_config_service,
        models,
        auth_service.clone(),
        user_service,
    ));
    Arc::new(HttpApiState::from_application_service(
        app,
        event_service,
        base_config,
        auth_service,
        auth_disabled,
        lifecycle,
    ))
}

fn test_state(auth_disabled: bool) -> Arc<HttpApiState> {
    test_state_with_lifecycle(auth_disabled, Arc::new(ServerLifecycle::new()))
}

fn test_state_with_config(
    config: talos_types::TalosConfig,
    auth_disabled: bool,
) -> Arc<HttpApiState> {
    let (auth_service, user_service) = test_auth_services();
    let event_store = Arc::new(RecordingEventStore::default());
    build_test_state_with_lifecycle(
        Arc::new(MockGateway),
        Arc::new(MockSessionService),
        event_service_from_store(event_store.clone()),
        event_store,
        config,
        Arc::new(MockLlmProvider),
        auth_service,
        user_service,
        auth_disabled,
        Arc::new(ServerLifecycle::new()),
    )
}

fn event_service_from_store(store: Arc<RecordingEventStore>) -> Arc<SessionEventService> {
    let event_store: Arc<dyn SessionEventStore> = store.clone();
    let active_turn_store: Arc<dyn ActiveTurnStore> = store;
    Arc::new(SessionEventService::new(
        event_store,
        active_turn_store,
        SessionEventRetentionConfig::default(),
    ))
}

fn test_state_with_event_store(
    auth_disabled: bool,
    store: Arc<RecordingEventStore>,
) -> Arc<HttpApiState> {
    test_state_with_gateway_event_store_config(
        Arc::new(MockGateway),
        auth_disabled,
        store,
        test_config(),
    )
}

fn test_state_with_gateway_event_store_config(
    gateway: Arc<dyn Gateway>,
    auth_disabled: bool,
    store: Arc<RecordingEventStore>,
    config: talos_types::TalosConfig,
) -> Arc<HttpApiState> {
    let llm_config_service: Arc<dyn LlmConfigService> = Arc::new(LlmConfigManager::new(Arc::new(
        InMemoryLlmConfigStore::new(config.llm.clone()),
    )));
    test_state_with_gateway_event_store_config_and_lifecycle(
        gateway,
        auth_disabled,
        store,
        config,
        Arc::new(ServerLifecycle::new()),
        llm_config_service,
    )
}

struct LegacySessionServiceRuntime {
    service: Arc<dyn SessionService>,
}

impl LegacySessionServiceRuntime {
    fn new(service: Arc<dyn SessionService>) -> Self {
        Self { service }
    }
}

#[async_trait::async_trait]
impl SessionRuntime for LegacySessionServiceRuntime {
    async fn resolve_target_agent(
        &self,
        context: &TalosContext,
    ) -> talos_error::TalosResult<String> {
        self.service.resolve_target_agent(context).await
    }

    async fn compact_session(&self, session_id: &str) -> talos_error::TalosResult<TalosSession> {
        self.service.compact_session(session_id).await
    }

    async fn clear_session(&self, session_id: &str) -> talos_error::TalosResult<TalosSession> {
        self.service.clear_session(session_id).await
    }

    async fn close_session(&self, session_id: &str) -> talos_error::TalosResult<()> {
        self.service.close_session(session_id).await
    }

    async fn delete_session(&self, session_id: &str) -> talos_error::TalosResult<SessionPatchAck> {
        self.service.delete_session(session_id).await
    }

    async fn cancel_active_turn(&self, session_id: &str) -> talos_error::TalosResult<()> {
        self.service.cancel_active_turn(session_id).await
    }

    async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> talos_error::TalosResult<()> {
        self.service
            .submit_tool_approval(session_id, call_id, tool_name, approved, reason)
            .await
    }

    async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> talos_error::TalosResult<()> {
        self.service
            .submit_dynamic_tool_result(session_id, call_id, tool_name, output, success)
            .await
    }
}

struct LegacySessionStore {
    service: Arc<dyn SessionService>,
}

impl LegacySessionStore {
    fn new(service: Arc<dyn SessionService>) -> Self {
        Self { service }
    }

    fn to_record(&self, session: TalosSession) -> talos_error::TalosResult<SessionRecord> {
        Ok(SessionRecord::from_session(
            &session,
            session.config.llm.model.as_str(),
        ))
    }
}

#[async_trait::async_trait]
impl SessionStore for LegacySessionStore {
    async fn create_session(
        &self,
        context: &TalosContext,
        _agent_id: &str,
        _default_model: &str,
        requested_model: Option<&str>,
        conf: Option<SessionConf>,
        _dm_scope: Option<String>,
        project_id: Option<String>,
        current_directory: Option<std::path::PathBuf>,
    ) -> talos_error::TalosResult<SessionRecord> {
        let session = self
            .service
            .create_session(
                context.clone(),
                requested_model.map(str::to_string),
                conf,
                project_id,
                current_directory,
            )
            .await?;
        self.to_record(session)
    }

    async fn resolve_or_create_session(
        &self,
        context: &TalosContext,
        agent_id: &str,
        default_model: &str,
        requested_model: Option<&str>,
        dm_scope: Option<String>,
        project_id: Option<String>,
        current_directory: Option<std::path::PathBuf>,
    ) -> talos_error::TalosResult<SessionRecord> {
        self.create_session(
            context,
            agent_id,
            default_model,
            requested_model,
            None,
            dm_scope,
            project_id,
            current_directory,
        )
        .await
    }

    async fn get_session_record(
        &self,
        session_id: &str,
    ) -> talos_error::TalosResult<Option<SessionRecord>> {
        match self.service.get_session(session_id).await? {
            Some(session) => Ok(Some(self.to_record(session)?)),
            None => Ok(None),
        }
    }

    async fn list_session_records(
        &self,
        query: &SessionQuery,
    ) -> talos_error::TalosResult<Vec<SessionRecord>> {
        let mut records = Vec::new();
        for session in self.service.list_sessions(query.clone()).await? {
            records.push(self.to_record(session)?);
        }
        Ok(records)
    }

    async fn count_session_records(&self, query: &SessionQuery) -> talos_error::TalosResult<usize> {
        self.service.count_sessions(query.clone()).await
    }

    async fn patch_session_record(
        &self,
        session_id: &str,
        patch: &SessionPatch,
    ) -> talos_error::TalosResult<SessionPatchAck> {
        self.service.patch_session(session_id, patch.clone()).await
    }

    async fn reset_session_record(
        &self,
        session_id: &str,
    ) -> talos_error::TalosResult<SessionPatchAck> {
        self.service.delete_session(session_id).await
    }

    async fn delete_session_record(
        &self,
        session_id: &str,
    ) -> talos_error::TalosResult<SessionPatchAck> {
        self.service.delete_session(session_id).await
    }

    async fn update_session_title(
        &self,
        _session_id: &str,
        _title: &str,
    ) -> talos_error::TalosResult<()> {
        Ok(())
    }

    async fn record_session_progress(
        &self,
        _session_id: &str,
        _message_delta: u64,
        _usage: &talos_types::talos::TalosUsage,
        _model: Option<&str>,
    ) -> talos_error::TalosResult<()> {
        Ok(())
    }

    async fn list_projects(&self) -> talos_error::TalosResult<Vec<ProjectRecord>> {
        self.service.list_projects().await
    }

    async fn get_project(
        &self,
        project_id: &str,
    ) -> talos_error::TalosResult<Option<ProjectRecord>> {
        self.service.get_project(project_id).await
    }

    async fn create_project(
        &self,
        name: &str,
        directory: &std::path::PathBuf,
    ) -> talos_error::TalosResult<ProjectRecord> {
        self.service
            .create_project(name.to_string(), directory.clone(), String::new())
            .await
    }

    async fn create_project_for_owner(
        &self,
        name: &str,
        directory: &std::path::PathBuf,
        user_id: &str,
    ) -> talos_error::TalosResult<ProjectRecord> {
        self.service
            .create_project(name.to_string(), directory.clone(), user_id.to_string())
            .await
    }

    async fn patch_project(
        &self,
        project_id: &str,
        patch: &ProjectPatch,
    ) -> talos_error::TalosResult<Option<ProjectRecord>> {
        self.service.patch_project(project_id, patch.clone()).await
    }

    async fn delete_project(&self, project_id: &str) -> talos_error::TalosResult<()> {
        self.service.delete_project(project_id).await
    }
}

fn test_state_with_gateway_event_store_config_and_lifecycle(
    gateway: Arc<dyn Gateway>,
    auth_disabled: bool,
    store: Arc<RecordingEventStore>,
    config: talos_types::TalosConfig,
    lifecycle: Arc<ServerLifecycle>,
    llm_config_service: Arc<dyn LlmConfigService>,
) -> Arc<HttpApiState> {
    let (auth_service, user_service) = test_auth_services();
    build_test_state_with_lifecycle_and_config(
        gateway,
        Arc::new(MockSessionService),
        event_service_from_store(store.clone()),
        store,
        config,
        Arc::new(MockLlmProvider),
        auth_service,
        user_service,
        auth_disabled,
        vec![test_model_info()],
        llm_config_service,
        lifecycle,
    )
}

#[derive(Default)]
struct RecordingEventStore {
    events: std::sync::Mutex<Vec<SessionEventRecord>>,
    active_turns: std::sync::Mutex<std::collections::HashMap<String, ActiveTurnRecord>>,
    idempotency_records:
        std::sync::Mutex<std::collections::HashMap<(String, String), IdempotencyRecord>>,
}

#[async_trait::async_trait]
impl SessionEventStore for RecordingEventStore {
    async fn append_event(&self, event: NewSessionEvent) -> TalosResult<SessionEventRecord> {
        let mut events = self.events.lock().unwrap();
        let event_seq = (events.len() as i64) + 1;
        let record = SessionEventRecord {
            event_seq,
            event_id: SessionEventId::from_seq(event_seq),
            session_id: event.session_id,
            turn_id: event.turn_id,
            message_id: event.message_id,
            sequence: event.sequence,
            event_type: event.event_type,
            payload: event.payload,
            created_at: event.created_at,
        };
        events.push(record.clone());
        Ok(record)
    }

    async fn list_events_after(
        &self,
        session_id: &str,
        after_event_id: Option<&SessionEventId>,
        limit: usize,
    ) -> TalosResult<Vec<SessionEventRecord>> {
        let after = match after_event_id {
            Some(event_id) => event_id.event_seq()?,
            None => 0,
        };
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.session_id == session_id && event.event_seq > after)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn latest_event_id(&self, session_id: &str) -> TalosResult<Option<SessionEventId>> {
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|event| event.session_id == session_id)
            .map(|event| event.event_id.clone()))
    }

    async fn oldest_event_id(&self, session_id: &str) -> TalosResult<Option<SessionEventId>> {
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .find(|event| event.session_id == session_id)
            .map(|event| event.event_id.clone()))
    }

    async fn delete_completed_events_before(
        &self,
        _before_timestamp_ms: i64,
        _limit: usize,
    ) -> TalosResult<u64> {
        Ok(0)
    }

    async fn delete_overflow_events_per_session(
        &self,
        _max_events_per_session: usize,
        _limit: usize,
    ) -> TalosResult<u64> {
        Ok(0)
    }
}

#[async_trait::async_trait]
impl ActiveTurnStore for RecordingEventStore {
    async fn upsert_active_turn(&self, update: ActiveTurnUpdate) -> TalosResult<()> {
        self.active_turns.lock().unwrap().insert(
            update.session_id.clone(),
            ActiveTurnRecord {
                session_id: update.session_id,
                turn_id: update.turn_id,
                message_id: update.message_id,
                status: update.status,
                last_event_id: update.last_event_id,
                pending_user_action: update.pending_user_action,
                updated_at: update.updated_at,
            },
        );
        Ok(())
    }

    async fn get_active_turn(&self, session_id: &str) -> TalosResult<Option<ActiveTurnRecord>> {
        Ok(self.active_turns.lock().unwrap().get(session_id).cloned())
    }

    async fn clear_active_turn(&self, session_id: &str) -> TalosResult<()> {
        self.active_turns.lock().unwrap().remove(session_id);
        Ok(())
    }
}

#[async_trait::async_trait]
impl MessageIdempotencyStore for RecordingEventStore {
    async fn get(
        &self,
        session_id: &str,
        idempotency_key: &str,
    ) -> TalosResult<Option<IdempotencyRecord>> {
        Ok(self
            .idempotency_records
            .lock()
            .unwrap()
            .get(&(session_id.to_string(), idempotency_key.to_string()))
            .cloned())
    }

    async fn reserve(
        &self,
        session_id: &str,
        idempotency_key: &str,
        request_hash: &str,
        turn_id: &str,
        now_ms: i64,
        expires_at_ms: Option<i64>,
    ) -> TalosResult<IdempotencyReservation> {
        let mut records = self.idempotency_records.lock().unwrap();
        let key = (session_id.to_string(), idempotency_key.to_string());
        if let Some(record) = records.get(&key) {
            if record
                .expires_at
                .is_some_and(|expires_at| expires_at <= now_ms)
            {
                records.remove(&key);
            } else {
                return Ok(IdempotencyReservation::Existing(record.clone()));
            }
        }
        let record = IdempotencyRecord {
            session_id: session_id.to_string(),
            idempotency_key: idempotency_key.to_string(),
            request_hash: request_hash.to_string(),
            turn_id: turn_id.to_string(),
            message_id: None,
            status: IdempotencyStatus::Pending,
            ack: None,
            created_at: now_ms,
            updated_at: now_ms,
            expires_at: expires_at_ms,
        };
        records.insert(key, record.clone());
        Ok(IdempotencyReservation::Reserved(record))
    }

    async fn complete(
        &self,
        session_id: &str,
        idempotency_key: &str,
        status: IdempotencyStatus,
        message_id: Option<&str>,
        ack: &MessageCommandAck,
        now_ms: i64,
    ) -> TalosResult<()> {
        let mut records = self.idempotency_records.lock().unwrap();
        let key = (session_id.to_string(), idempotency_key.to_string());
        if let Some(record) = records.get_mut(&key) {
            record.status = status;
            record.message_id = message_id.map(ToString::to_string);
            record.ack = Some(ack.clone());
            record.updated_at = now_ms;
        }
        Ok(())
    }

    async fn mark_stale_pending_failed(
        &self,
        older_than_ms: i64,
        limit: usize,
    ) -> TalosResult<u64> {
        let mut changed = 0;
        let mut records = self.idempotency_records.lock().unwrap();
        for record in records.values_mut() {
            if changed >= limit {
                break;
            }
            if record.status == IdempotencyStatus::Pending && record.updated_at < older_than_ms {
                record.status = IdempotencyStatus::FailedNoFirstEvent;
                changed += 1;
            }
        }
        Ok(changed as u64)
    }
}

fn test_auth_services() -> (Arc<dyn AuthService>, Arc<dyn UserService>) {
    let manager = Arc::new(AuthManager::new(
        Arc::new(InMemoryUserStore::default()),
        Arc::new(InMemoryUserTokenStore::default()),
        Arc::new(JwtAuth::new("test-secret")),
        true,
        AuthCacheConfig::default(),
    ));
    let auth_service: Arc<dyn AuthService> = manager.clone();
    let user_service: Arc<dyn UserService> = manager;
    (auth_service, user_service)
}

fn test_auth() -> RequestIdentity {
    RequestIdentity::mock_admin("static-token")
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

#[path = "tests/active_turn.rs"]
mod active_turn;
#[path = "tests/auth_lifecycle.rs"]
mod auth_lifecycle;
#[path = "tests/events.rs"]
mod events;
#[path = "tests/handlers.rs"]
mod handlers;
#[path = "tests/messages.rs"]
mod messages;
