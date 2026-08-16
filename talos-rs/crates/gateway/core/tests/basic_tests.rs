use async_trait::async_trait;
use opendal::{services::Memory, Operator};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use talos_auth::{AuthService, RequestIdentity, UserDto, UserRole, UserService};
use talos_config::{ConfigStore, InMemoryLlmConfigStore, ProviderModelConfig};
use talos_coordinator::SessionRuntime;
use talos_error::TalosResult;
use talos_gateway::{Gateway, ServiceGateway};
use talos_service::{Actor, AgentDispatchTurnExecutor, FileService, MemoryService, TalosService};
use talos_service::{TimelineEventRetentionConfig, TimelineEventService};
use talos_storage::SqliteStorage;
use talos_store::file::DatabaseFileStore;
use talos_store::memory::MemoryStore;
use talos_store::migration::run_migrations;
use talos_store::profile::{DatabaseProfileStore, ProfileStore};
use talos_store::project::{ProjectStore, StorageProjectStore};
use talos_store::session::{
    ActiveTurnStore, DatabaseSessionStore, MessageIdempotencyStore, SessionStore,
    TimelineEventStore,
};
use talos_types::agent_dispatch::{AgentDispatchInputParams, AgentDispatchParams};
use talos_types::talos::{
    AgentEvent, AgentEventPayload, AgentEventType, TalosContext, TalosInputItem, TalosProvider,
    TalosSourceType, TalosTurnStartedEvent,
};
use talos_types::{
    AgentDispatchRuntime, ConfigGetRequest, ConfigPath, ConfigUpdateRequest, ConfigValue,
    SessionPatchRequest, StartTurnRequest, TalosConfig, TalosSession, TokenUsage,
    TurnCommandStatus,
};
use tokio::sync::mpsc;

struct RecordingHandler;
struct TestAuthService;

#[async_trait]
impl AgentDispatchRuntime for RecordingHandler {
    async fn validate_agent_id(&self, _agent_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn handle(
        &self,
        _request: AgentDispatchParams,
    ) -> TalosResult<talos_types::agent_dispatch::AgentDispatchResult> {
        unreachable!("gateway start_turn should use stream execution")
    }

    async fn handle_stream(
        &self,
        request: AgentDispatchParams,
    ) -> TalosResult<mpsc::Receiver<AgentEvent>> {
        let (tx, rx) = mpsc::channel(1);
        tokio::spawn(async move {
            let turn_id = request.turn_id.clone();
            let _ = tx
                .send(AgentEvent {
                    event_seq: None,
                    event_id: None,
                    created_at_ms: None,
                    event_type: AgentEventType::TurnStarted,
                    turn_id: turn_id.clone(),
                    message_id: request.session.context.message_id,
                    session_id: request.session.context.session_id,
                    sequence: 0,
                    payload: AgentEventPayload::TurnStarted(TalosTurnStartedEvent {
                        request_id: "req_test".to_string(),
                        turn_id,
                        started_at_ms: 0,
                    }),
                })
                .await;
        });
        Ok(rx)
    }

    async fn submit_input(&self, _params: AgentDispatchInputParams) -> TalosResult<()> {
        Ok(())
    }
}

#[async_trait]
impl SessionRuntime for RecordingHandler {
    async fn resolve_target_agent(&self, _context: &TalosContext) -> TalosResult<String> {
        Ok("default".to_string())
    }

    async fn compact_session(
        &self,
        _params: talos_coordinator::AgentSessionCompactionParams,
    ) -> TalosResult<TokenUsage> {
        Ok(TokenUsage::default())
    }

    async fn clear_session(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn close_session(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn prepare_delete_session(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn cancel_active_turn(
        &self,
        _params: talos_types::CancelActiveTurnParams,
    ) -> TalosResult<()> {
        Ok(())
    }
}

#[async_trait]
impl AuthService for TestAuthService {
    async fn logout(&self, _actor: &RequestIdentity) -> TalosResult<()> {
        Ok(())
    }

    async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity> {
        Ok(RequestIdentity {
            user_id: token.to_string(),
            username: None,
            role: talos_auth::UserRole::User,
            auth_disabled: false,
            access_token: Some(token.to_string()),
        })
    }
}

#[async_trait]
impl UserService for TestAuthService {
    async fn get_current_user(&self, actor: &RequestIdentity) -> TalosResult<UserDto> {
        Ok(UserDto {
            user_id: actor.user_id.clone(),
            username: actor
                .username
                .clone()
                .unwrap_or_else(|| actor.user_id.clone()),
            email: None,
            role: actor.role,
            r#type: talos_auth::UserType::Free,
            disabled: false,
            created_at: 0,
            updated_at: 0,
        })
    }
}

async fn test_gateway() -> ServiceGateway {
    let config = test_config();
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    let migration_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite");
    run_migrations(storage.pool(), &migration_dir)
        .await
        .unwrap();
    let profile_store: Arc<dyn ProfileStore> =
        Arc::new(DatabaseProfileStore::new(storage.pool().clone()));
    let store = Arc::new(DatabaseSessionStore::new(storage.clone()).await.unwrap());
    let file_store = Arc::new(DatabaseFileStore::new(storage.pool().clone()));
    let project_store_impl = Arc::new(StorageProjectStore::new(storage).await.unwrap());
    let session_store: Arc<dyn SessionStore> = store.clone();
    let project_store: Arc<dyn ProjectStore> = project_store_impl.clone();
    let files = Arc::new(
        FileService::new(
            file_store,
            project_store.clone(),
            BTreeMap::from([(
                "local".to_string(),
                Operator::new(Memory::default()).unwrap().finish(),
            )]),
            talos_config::FileStorageConfig::default(),
        )
        .unwrap(),
    );
    let event_store: Arc<dyn TimelineEventStore> = store.clone();
    let active_turn_store: Arc<dyn ActiveTurnStore> = store.clone();
    let idempotency_store: Arc<dyn MessageIdempotencyStore> = store.clone();
    let event_service = Arc::new(TimelineEventService::new(
        event_store,
        active_turn_store,
        store.clone() as Arc<dyn talos_store::session::UserActionStore>,
        TimelineEventRetentionConfig::default(),
    ));
    let handler = Arc::new(RecordingHandler);
    let turn_executor = Arc::new(AgentDispatchTurnExecutor::new(
        handler.clone() as Arc<dyn AgentDispatchRuntime>
    ));
    let session_runtime: Arc<dyn SessionRuntime> = handler;
    let config_store: Arc<dyn ConfigStore> =
        Arc::new(InMemoryLlmConfigStore::new(config.llm.clone()));
    let auth = Arc::new(TestAuthService);
    let auth_service: Arc<dyn AuthService> = auth.clone();
    let user_service: Arc<dyn UserService> = auth;
    let memory = MemoryService::new(
        config.clone(),
        Arc::new(MemoryStore::in_memory().await.unwrap()),
    )
    .unwrap();
    let service = Arc::new(TalosService::new_with_session_store(
        turn_executor,
        session_runtime,
        session_store,
        Arc::new(talos_store::session::FileSessionArtifactStore::default()),
        project_store,
        profile_store,
        files,
        event_service,
        idempotency_store,
        config,
        memory,
        config_store,
        Vec::new(),
        auth_service,
        user_service,
    ));
    ServiceGateway::new(service)
}

fn test_config() -> TalosConfig {
    let mut config = TalosConfig::default();
    config.llm.provider = "anthropic".to_string();
    config.llm.model = "claude-3-5-sonnet".to_string();
    config.llm.anthropic.models = vec![ProviderModelConfig {
        id: config.llm.model.clone(),
        name: "Claude 3.5 Sonnet".to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }];
    config
}

fn request_context() -> TalosContext {
    TalosContext {
        source_type: TalosSourceType::Http,
        provider: TalosProvider::Http,
        user_id: "user-1".to_string(),
        message_id: "msg-1".to_string(),
        trace_id: "trace-1".to_string(),
        channel_id: None,
        thread_id: None,
        project_root: None,
        environment: None,
    }
}

#[tokio::test]
async fn gateway_start_turn_delegates_to_service() {
    let gateway = test_gateway().await;
    let response = gateway
        .start_turn(
            &Actor::regular("user-1"),
            StartTurnRequest {
                session_id: "session-1".to_string(),
                input: vec![TalosInputItem::Text {
                    text: "hello".to_string(),
                }],
                workspace_id: None,
                context: Some(request_context()),
                model_selection: None,
                user_permission_mode: None,
                project_id: None,
                current_directory: None,
                client_request_id: Some("client-1".to_string()),
            },
            Some("client-1".to_string()),
        )
        .await
        .expect("start turn should delegate to application service");

    assert_eq!(response.session_id, "session-1");
    assert_eq!(response.status, TurnCommandStatus::Accepted);
}

#[tokio::test]
async fn gateway_start_turn_rejects_invalid_request() {
    let gateway = test_gateway().await;
    let result = gateway
        .start_turn(
            &Actor::regular("user-1"),
            StartTurnRequest {
                session_id: "session-1".to_string(),
                input: Vec::new(),
                workspace_id: None,
                context: Some(request_context()),
                model_selection: None,
                user_permission_mode: None,
                project_id: None,
                current_directory: None,
                client_request_id: None,
            },
            None,
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn gateway_config_requires_admin_for_regular_auth() {
    let gateway = test_gateway().await;

    let read_error = gateway
        .get_config(
            &Actor::regular("user-1"),
            ConfigGetRequest {
                path: ConfigPath::WebSearch,
            },
        )
        .await
        .expect_err("regular users should not read global config");
    assert!(read_error.is_code(talos_error::ErrorCode::PermissionDenied));

    let update_error = gateway
        .update_config(
            &Actor::regular("user-1"),
            ConfigUpdateRequest {
                value: ConfigValue::WebSearch(Box::default()),
            },
        )
        .await
        .expect_err("regular users should not update global config");
    assert!(update_error.is_code(talos_error::ErrorCode::PermissionDenied));
}

#[tokio::test]
async fn gateway_config_allows_admin_and_auth_disabled() {
    let gateway = test_gateway().await;

    gateway
        .get_config(
            &Actor::regular("admin-1").with_role(UserRole::Admin),
            ConfigGetRequest {
                path: ConfigPath::WebSearch,
            },
        )
        .await
        .expect("admin should read global config");

    gateway
        .update_config(
            &Actor::auth_disabled(),
            ConfigUpdateRequest {
                value: ConfigValue::WebSearch(Box::default()),
            },
        )
        .await
        .expect("auth-disabled mode should update global config");
}

#[tokio::test]
async fn gateway_patch_session_rejects_missing_session_id() {
    let gateway = test_gateway().await;
    let result = gateway
        .patch_session(
            &Actor::regular("user-1"),
            SessionPatchRequest {
                session_id: String::new(),
                conf: None,
                project_id: None,
                current_directory: None,
            },
        )
        .await;

    assert!(result
        .as_ref()
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
}

#[tokio::test]
async fn gateway_start_is_available() {
    let gateway = test_gateway().await;
    gateway.start().await.expect("gateway should start");
}
