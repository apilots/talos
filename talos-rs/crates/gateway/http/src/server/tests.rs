use super::*;
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::Value;
use std::sync::Mutex;
use talos_application_service::TalosApplicationService;
use talos_auth::{
    AdminCreateUserRequest, AdminPatchUserRequest, AuthResponse, AuthService,
    CreateUserTokenRequest, CreateUserTokenResult, LoginRequest, RegisterRequest, RequestIdentity,
    UpdateCurrentUserRequest, UserDto, UserService, UserTokenMeta,
};
use talos_config::{InMemoryLlmConfigStore, LlmConfigManager, LlmConfigService};
use talos_error::{TalosError, TalosResult};
use talos_gateway::Gateway;
use talos_gateway::GatewayRequest;
use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Usage};
use talos_session_store::{
    ActiveTurnRecord, ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, IdempotencyRecord,
    IdempotencyReservation, IdempotencyStatus, MessageCommandAck, MessageIdempotencyStore,
    PendingDynamicToolResult, PendingUserAction, SessionStore, TimelineEventId,
    TimelineEventRetentionConfig, TimelineEventService, TimelineEventStore,
};
use talos_types::talos::{
    StopReason, TalosContext, TalosInputItem, TalosItemDeltaEvent, TalosItemStartedEvent,
    TalosMessageRequest, TalosOutputItem, TalosProvider, TalosReasoning, TalosSourceType,
    TalosTimelineContent, TalosTimelineDelta, TalosTimelineEvent, TalosTimelineEventPayload,
    TalosTimelineEventType, TalosTimelineItem, TalosTimelineItemKind, TalosTimelineItemStatus,
    TalosTimelinePhase, TalosToolProgressKind, TalosTurnCompletedEvent, TalosTurnStartedEvent,
    TalosUsage,
};
use talos_types::{
    ModelInfo, ProjectPatch, ProjectRecord, SessionConf, SessionPatch, SessionPatchAck,
    SessionQuery, SessionRecord, SessionRuntime, SessionService, TalosSession,
};
use tower::util::ServiceExt;

use crate::server::admin_users::{delete_admin_session_handler, list_admin_sessions_handler};
use crate::server::events::{get_active_turn_handler, subscribe_timeline_events_handler};
use crate::server::messages::{
    retry_message_attempt_handler, session_message_handler, submit_message_command,
};
use crate::server::misc::get_llm_handler;
use crate::server::user_profile::{
    create_user_token_handler, list_admin_users_handler, list_user_tokens_handler,
    local_auth_removed_handler,
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
    event_service: Arc<TimelineEventService>,
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
    event_service: Arc<TimelineEventService>,
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
        Arc::new(SessionServiceRuntimeAdapter::new(session_service.clone())),
        Arc::new(SessionStoreAdapter::new(session_service)),
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

fn event_service_from_store(store: Arc<RecordingEventStore>) -> Arc<TimelineEventService> {
    let event_store: Arc<dyn TimelineEventStore> = store.clone();
    let active_turn_store: Arc<dyn ActiveTurnStore> = store;
    Arc::new(TimelineEventService::new(
        event_store,
        active_turn_store,
        TimelineEventRetentionConfig::default(),
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

struct SessionServiceRuntimeAdapter {
    service: Arc<dyn SessionService>,
}

impl SessionServiceRuntimeAdapter {
    fn new(service: Arc<dyn SessionService>) -> Self {
        Self { service }
    }
}

#[async_trait::async_trait]
impl SessionRuntime for SessionServiceRuntimeAdapter {
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

struct SessionStoreAdapter {
    service: Arc<dyn SessionService>,
}

impl SessionStoreAdapter {
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
impl SessionStore for SessionStoreAdapter {
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
    events: std::sync::Mutex<Vec<TalosTimelineEvent>>,
    active_turns: std::sync::Mutex<std::collections::HashMap<String, ActiveTurnRecord>>,
    idempotency_records:
        std::sync::Mutex<std::collections::HashMap<(String, String), IdempotencyRecord>>,
}

#[async_trait::async_trait]
impl TimelineEventStore for RecordingEventStore {
    async fn append_event(&self, mut event: TalosTimelineEvent) -> TalosResult<TalosTimelineEvent> {
        let mut events = self.events.lock().unwrap();
        let event_seq = (events.len() as i64) + 1;
        event.event_seq = Some(event_seq);
        event.event_id = Some(TimelineEventId::from_seq(event_seq).to_string());
        event.created_at_ms = Some(event.created_at_ms.unwrap_or(1000 + event_seq));
        events.push(event.clone());
        Ok(event)
    }

    async fn list_events_after(
        &self,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<Vec<TalosTimelineEvent>> {
        let after = match after_event_id {
            Some(event_id) => event_id.event_seq()?,
            None => 0,
        };
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| {
                event.session_id == session_id && event.event_seq.unwrap_or_default() > after
            })
            .take(limit)
            .cloned()
            .collect())
    }

    async fn latest_event_id(&self, session_id: &str) -> TalosResult<Option<TimelineEventId>> {
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|event| event.session_id == session_id)
            .and_then(|event| event.event_id.clone())
            .map(TimelineEventId::from))
    }

    async fn oldest_event_id(&self, session_id: &str) -> TalosResult<Option<TimelineEventId>> {
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .find(|event| event.session_id == session_id)
            .and_then(|event| event.event_id.clone())
            .map(TimelineEventId::from))
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

#[derive(Default)]
struct FakeAuthService {
    tokens: Mutex<Vec<UserTokenMeta>>,
}

impl FakeAuthService {
    fn user_from_actor(actor: &RequestIdentity) -> UserDto {
        UserDto {
            user_id: actor.user_id.clone(),
            username: actor
                .username
                .clone()
                .unwrap_or_else(|| actor.user_id.clone()),
            email: None,
            role: actor.role,
            disabled: false,
            created_at: 1,
            updated_at: 1,
        }
    }
}

#[async_trait]
impl AuthService for FakeAuthService {
    async fn register(&self, _input: RegisterRequest) -> TalosResult<AuthResponse> {
        Err(TalosError::AuthError(
            "local auth is not available in tests".to_string(),
        ))
    }

    async fn login(&self, _input: LoginRequest) -> TalosResult<AuthResponse> {
        Err(TalosError::AuthError(
            "local auth is not available in tests".to_string(),
        ))
    }

    async fn logout(&self, _actor: &RequestIdentity) -> TalosResult<()> {
        Ok(())
    }

    async fn create_token(
        &self,
        _actor: &RequestIdentity,
        name: &str,
    ) -> TalosResult<CreateUserTokenResult> {
        let token_id = format!("token-{}", self.tokens.lock().unwrap().len() + 1);
        let raw_token = format!("tls_{}", "a".repeat(32));
        let prefix = raw_token.chars().take(12).collect::<String>();
        self.tokens.lock().unwrap().push(UserTokenMeta {
            token_id: token_id.clone(),
            name: name.to_string(),
            prefix: prefix.clone(),
            created_at: 1,
            last_used_at: None,
            expires_at: None,
        });
        Ok(CreateUserTokenResult {
            token_id,
            raw_token,
            prefix,
        })
    }

    async fn list_tokens(&self, _actor: &RequestIdentity) -> TalosResult<Vec<UserTokenMeta>> {
        Ok(self.tokens.lock().unwrap().clone())
    }

    async fn revoke_token(&self, _actor: &RequestIdentity, token_id: &str) -> TalosResult<()> {
        self.tokens
            .lock()
            .unwrap()
            .retain(|token| token.token_id != token_id);
        Ok(())
    }

    async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity> {
        Ok(RequestIdentity::mock_admin(token))
    }
}

#[async_trait]
impl UserService for FakeAuthService {
    async fn get_current_user(&self, actor: &RequestIdentity) -> TalosResult<UserDto> {
        Ok(Self::user_from_actor(actor))
    }

    async fn patch_current_user(
        &self,
        actor: &RequestIdentity,
        patch: UpdateCurrentUserRequest,
    ) -> TalosResult<UserDto> {
        let mut user = Self::user_from_actor(actor);
        if let Some(username) = patch.username {
            user.username = username;
        }
        Ok(user)
    }

    async fn list_users(&self, actor: &RequestIdentity) -> TalosResult<Vec<UserDto>> {
        if actor.auth_disabled {
            return Ok(Vec::new());
        }
        Ok(vec![Self::user_from_actor(actor)])
    }

    async fn create_user(
        &self,
        _actor: &RequestIdentity,
        request: AdminCreateUserRequest,
    ) -> TalosResult<UserDto> {
        Ok(UserDto {
            user_id: format!("user-{}", request.username),
            username: request.username,
            email: request.email,
            role: request.role,
            disabled: request.disabled.unwrap_or(false),
            created_at: 1,
            updated_at: 1,
        })
    }

    async fn patch_user(
        &self,
        actor: &RequestIdentity,
        _user_id: &str,
        patch: AdminPatchUserRequest,
    ) -> TalosResult<UserDto> {
        let mut user = Self::user_from_actor(actor);
        if let Some(username) = patch.username {
            user.username = username;
        }
        if let Some(email) = patch.email {
            user.email = Some(email);
        }
        if let Some(role) = patch.role {
            user.role = role;
        }
        if let Some(disabled) = patch.disabled {
            user.disabled = disabled;
        }
        Ok(user)
    }

    async fn delete_user(&self, _actor: &RequestIdentity, _user_id: &str) -> TalosResult<()> {
        Ok(())
    }
}

fn test_auth_services() -> (Arc<dyn AuthService>, Arc<dyn UserService>) {
    let service = Arc::new(FakeAuthService::default());
    let auth_service: Arc<dyn AuthService> = service.clone();
    let user_service: Arc<dyn UserService> = service;
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
