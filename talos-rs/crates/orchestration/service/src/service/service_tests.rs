use super::*;
use crate::access::CreateSessionRequest;
use crate::TimelineEventRetentionConfig;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use talos_auth::{AuthService, RequestIdentity, UserDto, UserService};
use talos_config::{InMemoryLlmConfigStore, LlmConfigManager, LlmConfigService};
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Usage};
use talos_storage::SqliteStorage;
use talos_store::project::StorageProjectStore;
use talos_store::session::{
    ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, DatabaseSessionStore,
    IdempotencyReservation, MessageIdempotencyStore, SessionStore, TimelineEventStore,
};
use talos_types::agent_dispatch::{AgentDispatchInputParams, AgentDispatchParams};
use talos_types::talos::{
    AgentEvent, TalosContext, TalosInputItem, TalosProvider, TalosSourceType,
};

enum TestTurnExecutorMode {
    Timeout,
    Closed,
}

struct TestTurnExecutor {
    mode: TestTurnExecutorMode,
}

struct TestSessionRuntime {
    cancel_mode: TestCancelMode,
    cancelled_turn_ids: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone, Copy)]
enum TestCancelMode {
    Ok,
    MissingRuntimeTurn,
}

struct TestLlmProvider;
struct TestAuthService;

#[async_trait]
impl TurnExecutor for TestTurnExecutor {
    async fn start_turn_stream(
        &self,
        _request: AgentDispatchParams,
    ) -> TalosResult<tokio::sync::mpsc::Receiver<AgentEvent>> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        match self.mode {
            TestTurnExecutorMode::Timeout => {
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    drop(tx);
                });
            }
            TestTurnExecutorMode::Closed => drop(tx),
        }
        Ok(rx)
    }

    async fn submit_input(&self, _params: AgentDispatchInputParams) -> TalosResult<()> {
        Ok(())
    }
}

#[async_trait]
impl SessionRuntime for TestSessionRuntime {
    async fn resolve_target_agent(&self, _context: &TalosContext) -> TalosResult<String> {
        Ok("default".to_string())
    }

    async fn compact_session(
        &self,
        _session: talos_types::TalosSession,
    ) -> TalosResult<talos_types::TokenUsage> {
        unreachable!("not used in this test")
    }

    async fn clear_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        unreachable!("not used in this test")
    }

    async fn close_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        unreachable!("not used in this test")
    }

    async fn prepare_delete_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn cancel_active_turn(
        &self,
        params: talos_types::CancelActiveTurnParams,
    ) -> TalosResult<()> {
        self.cancelled_turn_ids.lock().unwrap().push(params.turn_id);
        match self.cancel_mode {
            TestCancelMode::Ok => Ok(()),
            TestCancelMode::MissingRuntimeTurn => Err(TalosError::ResourceNotFound(
                "active turn for session session-interrupt-runtime-missing".to_string(),
            )),
        }
    }
}

#[async_trait]
impl LlmProvider for TestLlmProvider {
    fn name(&self) -> &str {
        "test"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: String::new(),
            reasoning: Default::default(),
            model: "default".to_string(),
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

    fn default_model(&self) -> String {
        "default".to_string()
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

async fn test_application(
    turn_executor: Arc<dyn TurnExecutor>,
    config: TalosConfig,
) -> (TalosService, Arc<DatabaseSessionStore>) {
    test_application_with_runtime(
        turn_executor,
        Arc::new(TestSessionRuntime {
            cancel_mode: TestCancelMode::Ok,
            cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
        }),
        config,
    )
    .await
}

async fn test_application_with_runtime(
    turn_executor: Arc<dyn TurnExecutor>,
    session_runtime: Arc<dyn SessionRuntime>,
    config: TalosConfig,
) -> (TalosService, Arc<DatabaseSessionStore>) {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    let store = Arc::new(DatabaseSessionStore::new(storage.clone()).await.unwrap());
    let project_store = Arc::new(StorageProjectStore::new(storage).await.unwrap());
    let event_store: Arc<dyn TimelineEventStore> = store.clone();
    let active_turn_store: Arc<dyn ActiveTurnStore> = store.clone();
    let event_service = Arc::new(TimelineEventService::new(
        event_store,
        active_turn_store,
        TimelineEventRetentionConfig::default(),
    ));
    let idempotency_store: Arc<dyn MessageIdempotencyStore> = store.clone();
    let llm_config_service: Arc<dyn LlmConfigService> = Arc::new(LlmConfigManager::new(Arc::new(
        InMemoryLlmConfigStore::new(config.llm.clone()),
    )));
    let auth = Arc::new(TestAuthService);
    let auth_service: Arc<dyn AuthService> = auth.clone();
    let user_service: Arc<dyn UserService> = auth;
    let service = TalosService::new_with_session_store(
        turn_executor,
        session_runtime,
        store.clone(),
        project_store,
        event_service,
        idempotency_store,
        config,
        Arc::new(TestLlmProvider),
        llm_config_service,
        Vec::new(),
        auth_service,
        user_service,
    );
    (service, store)
}

fn request_context() -> TalosContext {
    TalosContext {
        source_type: TalosSourceType::Http,
        provider: TalosProvider::Http,
        user_id: "client-user".to_string(),
        message_id: "client-message".to_string(),
        trace_id: "trace-1".to_string(),
        channel_id: None,
        thread_id: None,
        project_root: None,
        environment: None,
    }
}

fn start_turn_request(session_id: &str, text: &str) -> StartTurnRequest {
    StartTurnRequest {
        session_id: session_id.to_string(),
        input: vec![TalosInputItem::Text {
            text: text.to_string(),
        }],
        workspace_id: None,
        context: Some(request_context()),
        model_selection: None,
        project_id: None,
        current_directory: None,
        client_request_id: None,
    }
}

fn create_session_params(
    session_id: &str,
    context: TalosContext,
    current_directory: Option<std::path::PathBuf>,
) -> CreateSessionRequest {
    CreateSessionRequest {
        session_id: session_id.to_string(),
        context,
        model_selection: None,
        workspace_id: None,
        project_id: None,
        current_directory,
    }
}

#[tokio::test]
async fn existing_session_start_turn_applies_model_selection_before_dispatch() {
    let mut config = TalosConfig::default();
    config.llm.anthropic.models = vec![talos_config::ProviderModelConfig {
        id: "turn-model".to_string(),
        name: "Turn Model".to_string(),
        context_window: 128_000,
        reasoning: true,
        reasoning_effort: talos_config::ReasoningEffort::Medium,
        speed: talos_config::ModelSpeed::Standard,
        input: vec![talos_config::ModelInputKind::Text],
        cost: talos_config::ModelCostConfig::default(),
    }];
    let (service, _store) = test_application(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        config,
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-model-ref", request_context(), None),
        )
        .await
        .expect("session should be created");

    let mut request = start_turn_request("session-model-ref", "hello");
    request.model_selection = Some(talos_types::ModelSelection::model_default(
        talos_types::ModelRef {
            provider: "anthropic".to_string(),
            id: "turn-model".to_string(),
        },
    ));
    let _ = service.start_turn(&actor, request, None).await;

    let session = service
        .get_session(&actor, "session-model-ref")
        .await
        .expect("session should still be readable");
    assert_eq!(session.config.llm.provider, "anthropic");
    assert_eq!(session.config.llm.model, "turn-model");
}

async fn assert_session_absent(store: &DatabaseSessionStore, session_id: &str) {
    assert!(store
        .get_session_record(session_id)
        .await
        .expect("session read should succeed")
        .is_none());
}

async fn assert_session_creation_rolled_back(
    service: &TalosService,
    store: &DatabaseSessionStore,
    session_id: &str,
    idempotency_key: Option<&str>,
) {
    assert_session_absent(store, session_id).await;
    assert!(service
        .history
        .snapshot(session_id)
        .await
        .expect("history snapshot should succeed")
        .messages
        .is_empty());
    let recovery = service
        .events
        .recover_timeline(session_id, None, 32)
        .await
        .expect("recovery should succeed");
    assert!(recovery.replay.events.is_empty());
    assert_eq!(
        recovery.active_turn.status,
        talos_types::TimelineActiveTurnStatus::Completed
    );
    assert!(service
        .events
        .get_active_turn(session_id)
        .await
        .expect("active turn lookup should succeed")
        .is_none());
    if let Some(key) = idempotency_key {
        assert!(store
            .get(session_id, key)
            .await
            .expect("idempotency lookup should succeed")
            .is_none());
        let reservation = store
            .reserve(
                session_id,
                key,
                "new-request-hash",
                "turn-new",
                chrono::Utc::now().timestamp_millis(),
                None,
            )
            .await
            .expect("idempotency key should be reusable after rollback");
        assert!(matches!(reservation, IdempotencyReservation::Reserved(_)));
    }
}

#[tokio::test]
async fn first_turn_timeout_does_not_leave_session_record() {
    let mut config = TalosConfig::default();
    config.agent.kernel.timeline_events.first_event_timeout_ms = 1;
    let (service, store) = test_application(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Timeout,
        }),
        config,
    )
    .await;

    let result = service
        .start_turn(
            &Actor::regular("member-user"),
            start_turn_request("session-timeout", "hello"),
            Some("timeout-key".to_string()),
        )
        .await;

    assert!(result.is_err());
    assert_session_creation_rolled_back(&service, &store, "session-timeout", Some("timeout-key"))
        .await;
}

#[tokio::test]
async fn first_turn_closed_stream_does_not_leave_session_record() {
    let (service, store) = test_application(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        TalosConfig::default(),
    )
    .await;

    let result = service
        .start_turn(
            &Actor::regular("member-user"),
            start_turn_request("session-closed", "hello"),
            Some("closed-key".to_string()),
        )
        .await;

    assert!(result.is_err());
    assert_session_creation_rolled_back(&service, &store, "session-closed", Some("closed-key"))
        .await;
}

#[tokio::test]
async fn first_turn_idempotency_conflict_does_not_leave_session_record() {
    let (service, store) = test_application(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        TalosConfig::default(),
    )
    .await;
    let reservation = store
        .reserve(
            "session-idempotency-conflict",
            "conflict-key",
            "different-request-hash",
            "turn-existing",
            1,
            None,
        )
        .await
        .expect("idempotency reserve should succeed");
    assert!(matches!(reservation, IdempotencyReservation::Reserved(_)));

    let result = service
        .start_turn(
            &Actor::regular("member-user"),
            start_turn_request("session-idempotency-conflict", "hello"),
            Some("conflict-key".to_string()),
        )
        .await;

    assert!(matches!(result, Err(TalosError::IdempotencyConflict(_))));
    assert_session_creation_rolled_back(
        &service,
        &store,
        "session-idempotency-conflict",
        Some("conflict-key"),
    )
    .await;
}

#[tokio::test]
async fn first_turn_active_turn_conflict_does_not_leave_session_record() {
    let (service, store) = test_application(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        TalosConfig::default(),
    )
    .await;
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "session-active-conflict".to_string(),
            turn_id: "turn-existing".to_string(),
            message_id: None,
            status: ActiveTurnStatus::Running,
            last_event_id: None,
            pending_user_action: None,
            updated_at: 1,
        })
        .await
        .expect("active turn insert should succeed");

    let result = service
        .start_turn(
            &Actor::regular("member-user"),
            start_turn_request("session-active-conflict", "hello"),
            None,
        )
        .await;

    assert!(result.is_err());
    assert_session_creation_rolled_back(&service, &store, "session-active-conflict", None).await;
}

#[tokio::test]
async fn interrupt_turn_without_active_turn_is_idempotent() {
    let (service, _store) = test_application_with_runtime(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        Arc::new(TestSessionRuntime {
            cancel_mode: TestCancelMode::MissingRuntimeTurn,
            cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
        }),
        TalosConfig::default(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-interrupt-idempotent", request_context(), None),
        )
        .await
        .expect("session should be created");

    let response = service
        .interrupt_turn(
            &actor,
            talos_types::InterruptTurnRequest {
                session_id: "session-interrupt-idempotent".to_string(),
            },
        )
        .await
        .expect("interrupt without active turn should be idempotent");

    assert!(response.success);
}

#[tokio::test]
async fn interrupt_turn_unknown_session_returns_not_found() {
    let (service, _store) = test_application_with_runtime(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        Arc::new(TestSessionRuntime {
            cancel_mode: TestCancelMode::MissingRuntimeTurn,
            cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
        }),
        TalosConfig::default(),
    )
    .await;

    let error = service
        .interrupt_turn(
            &Actor::regular("member-user"),
            talos_types::InterruptTurnRequest {
                session_id: "missing-session".to_string(),
            },
        )
        .await
        .expect_err("unknown session should be rejected");

    assert!(matches!(error, TalosError::ResourceNotFound(_)));
}

#[tokio::test]
async fn interrupt_turn_reports_missing_runtime_for_persisted_active_turn() {
    let cancelled_turn_ids = Arc::new(Mutex::new(Vec::new()));
    let (service, store) = test_application_with_runtime(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        Arc::new(TestSessionRuntime {
            cancel_mode: TestCancelMode::MissingRuntimeTurn,
            cancelled_turn_ids: cancelled_turn_ids.clone(),
        }),
        TalosConfig::default(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-interrupt-runtime-missing", request_context(), None),
        )
        .await
        .expect("session should be created");
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "session-interrupt-runtime-missing".to_string(),
            turn_id: "turn-existing".to_string(),
            message_id: Some("msg-existing".to_string()),
            status: ActiveTurnStatus::Running,
            last_event_id: None,
            pending_user_action: None,
            updated_at: 1,
        })
        .await
        .expect("active turn should be inserted");

    let error = service
        .interrupt_turn(
            &actor,
            talos_types::InterruptTurnRequest {
                session_id: "session-interrupt-runtime-missing".to_string(),
            },
        )
        .await
        .expect_err("runtime state mismatch should be reported");

    assert!(matches!(error, TalosError::ResourceNotFound(_)));
    assert_eq!(
        *cancelled_turn_ids.lock().unwrap(),
        vec!["turn-existing".to_string()]
    );
}
