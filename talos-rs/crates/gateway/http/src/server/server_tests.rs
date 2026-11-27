use super::*;
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use talos_agent::{
    AgentAdmission, AgentAdmissionPermit, AgentContext, AgentInputParams, AgentInterruptParams,
    AgentProgressEventReceiver, AgentRunExecution, AgentRunParams, AgentRunSettlement,
    AgentRunSettlementReceiver, AgentSessionContext,
};
use talos_agent_api::{
    ActiveTurnState, AgentEvent as CanonicalAgentEvent,
    AgentEventPayload as CanonicalAgentEventPayload, AgentInterruptOutcome, AgentRunMetrics,
    AgentTurnCompleted, AgentTurnRef, AgentTurnStarted,
};
use talos_auth::{AuthService, RequestIdentity, UserDto, UserService};
use talos_config::{ConfigStore, InMemoryLlmConfigStore};
use talos_coordinator::{AgentCoordinator, AgentSelectionParams};
use talos_error::{TalosError, TalosResult};
use talos_service::TalosService;
use talos_service::TimelineEventService;
use talos_store::profile::{
    ProfileDailyUsageSummary, ProfileModelUsageSummary, ProfileStore, ProfileUsageSummary,
    ProfileUsageUpdate,
};
use talos_store::project::MockProjectStore;
use talos_store::session::{AgentActiveTurnRecord, AgentExecutionStore, SessionStore};
use talos_types::talos::TokenUsage;
use talos_types::{
    DynamicToolResultParams, ModelInfo, PendingUserAction, SessionListRequest, UserActionParams,
    UserActionState,
};
use tower::util::ServiceExt;

use crate::server::events::{get_active_turn_handler, subscribe_timeline_events_handler};
use crate::server::message_queue::{add_queued_message_handler, list_message_queue_handler};
use crate::server::messages::{
    interrupt_turn_handler, retry_turn_handler, start_turn_handler, submit_turn_input_handler,
};
use crate::server::misc::{get_config_handler, get_llm_handler, put_config_handler};

#[path = "tests/support.rs"]
mod support;
use support::*;

#[path = "tests/recording_execution_store.rs"]
mod recording_execution_store;
use recording_execution_store::RecordingAgentExecutionStore;

fn expect_success<T>(response: talos_types::ApiResponse<T>) -> T {
    match response {
        talos_types::ApiResponse::Success { data } => data,
        talos_types::ApiResponse::Error { error } => {
            panic!("expected success response, got error: {error:?}")
        }
    }
}

fn pending_dynamic_action(action_id: &str, call_id: &str, tool_name: &str) -> PendingUserAction {
    PendingUserAction {
        action_id: action_id.to_string(),
        call_id: call_id.to_string(),
        item_id: None,
        state: UserActionState::Pending,
        params: UserActionParams::DynamicToolResult(DynamicToolResultParams {
            tool_name: tool_name.to_string(),
        }),
    }
}

#[derive(Deserialize)]
struct RejectionQuery {
    limit: u32,
}

async fn json_rejection_handler(Json(_body): Json<serde_json::Value>) {}

async fn query_rejection_handler(Query(query): Query<RejectionQuery>) {
    let _ = query.limit;
}

async fn path_rejection_handler(Path(_id): Path<u64>) {}

async fn assert_structured_invalid_request(response: axum::response::Response) {
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let payload: Value = serde_json::from_slice(&body).expect("payload should be valid json");
    assert_eq!(
        payload,
        serde_json::json!({
            "status": "error",
            "error": {
                "code": "invalidRequest",
                "message": "The request could not be parsed. Check its JSON, query parameters, and path values.",
                "localization": null
            }
        })
    );
}

#[tokio::test]
async fn v1_extractor_rejections_are_structured() {
    let app = Router::new()
        .route("/v1/json", post(json_rejection_handler))
        .route("/v1/query", get(query_rejection_handler))
        .route("/v1/path/:id", get(path_rejection_handler))
        .layer(middleware::from_fn(errors::normalize_extractor_rejection));

    let malformed_json = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/json")
                .header("content-type", "application/json")
                .body(Body::from("{"))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_structured_invalid_request(malformed_json).await;

    let malformed_query = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/query?limit=invalid")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_structured_invalid_request(malformed_query).await;

    let malformed_path = app
        .oneshot(
            Request::builder()
                .uri("/v1/path/invalid")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_structured_invalid_request(malformed_path).await;
}

struct EmptyProfileStore;

#[async_trait]
impl ProfileStore for EmptyProfileStore {
    async fn record_usage(&self, _update: ProfileUsageUpdate) -> TalosResult<()> {
        Ok(())
    }

    async fn discard_turn_usage(&self, _session_id: &str, _turn_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn delete_finalized_before(
        &self,
        _completed_before_ms: i64,
        _limit: usize,
    ) -> TalosResult<u64> {
        Ok(0)
    }

    async fn usage_summary(&self, _user_id: &str) -> TalosResult<ProfileUsageSummary> {
        Ok(ProfileUsageSummary {
            usage: TokenUsage::default(),
            usage_available: false,
            usage_session_count: 0,
            usage_started_at_ms: None,
        })
    }

    async fn daily_usage(
        &self,
        _user_id: &str,
        _start_date: &str,
        _end_date: &str,
    ) -> TalosResult<Vec<ProfileDailyUsageSummary>> {
        Ok(Vec::new())
    }

    async fn model_usage(
        &self,
        _user_id: &str,
        _limit: usize,
    ) -> TalosResult<Vec<ProfileModelUsageSummary>> {
        Ok(Vec::new())
    }
}

fn test_model_info() -> ModelInfo {
    ModelInfo::from_provider_model(
        "bailian",
        talos_types::ProviderModelConfig {
            reasoning_replay: Default::default(),
            reasoning_efforts: None,
            request_parameters: Default::default(),
            id: "qwen-plus".to_string(),
            name: "Qwen Plus".to_string(),
            context_window: 32_768,
            reasoning: true,
            reasoning_effort: Default::default(),
            speed: Default::default(),
            input: vec![talos_types::ModelInputKind::Text],
            cost: talos_types::ModelCostConfig::default(),
        },
    )
}

fn test_config() -> talos_types::TalosConfig {
    let mut config = talos_types::TalosConfig::default();
    let workspace_root =
        std::env::temp_dir().join(format!("talos-http-test-{}", uuid::Uuid::new_v4()));
    config = config.with_workspace_definition(talos_config::WorkspaceDefinition::new(
        talos_config::DEFAULT_WORKSPACE_ID,
        workspace_root,
    ));
    config.llm.provider = "bailian".to_string();
    config.llm.model = "qwen-plus".to_string();
    config.llm.bailian.models = vec![test_model_info().model];
    config
}

fn isolated_test_config() -> (talos_types::TalosConfig, std::path::PathBuf) {
    let config = test_config();
    let workspace_dir = config
        .workspace_root_prefix(config.default_workspace_id())
        .unwrap();
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should exist");
    (config, workspace_dir)
}

async fn test_state_with_lifecycle(
    auth_disabled: bool,
    lifecycle: Arc<ServerLifecycle>,
) -> Arc<HttpApiState> {
    let event_store = Arc::new(RecordingAgentExecutionStore::default());
    build_test_state(TestStateConfig {
        event_store,
        base_config: test_config(),
        auth_disabled,
        lifecycle,
        config_store: None,
    })
    .await
}

struct TestStateConfig {
    event_store: Arc<dyn AgentExecutionStore>,
    base_config: talos_types::TalosConfig,
    auth_disabled: bool,
    lifecycle: Arc<ServerLifecycle>,
    config_store: Option<Arc<dyn ConfigStore>>,
}

async fn build_test_state(config: TestStateConfig) -> Arc<HttpApiState> {
    let (product, _) = test_session_store().await;
    build_test_state_with_stores(
        config,
        product.clone(),
        Arc::new(MockProjectStore::new()),
        Some(product),
    )
    .await
}

async fn build_test_state_with_agent_execution_store(
    config: TestStateConfig,
    agent_execution_store: Arc<dyn AgentExecutionStore>,
) -> Arc<HttpApiState> {
    build_test_state_with_stores(
        config,
        agent_execution_store,
        Arc::new(MockProjectStore::new()),
        None,
    )
    .await
}

async fn build_test_state_with_stores(
    config: TestStateConfig,
    agent_execution_store: Arc<dyn AgentExecutionStore>,
    project_store: Arc<dyn talos_store::project::ProjectStore>,
    product_store: Option<Arc<dyn talos_store::session::SessionStore>>,
) -> Arc<HttpApiState> {
    let (auth_service, user_service) = test_auth_services();
    let event_service = event_service_from_store(config.event_store.clone());
    let config_store = config
        .config_store
        .unwrap_or_else(|| Arc::new(InMemoryLlmConfigStore::new(config.base_config.llm.clone())));
    let memory = talos_service::MemoryService::new(
        config.base_config.clone(),
        Arc::new(talos_store::memory::MemoryStore::in_memory().await.unwrap()),
    )
    .unwrap();
    memory.rebuild_known_scopes().await.unwrap();
    let files = test_file_service(
        project_store.clone(),
        config.base_config.storage.files.clone(),
    )
    .await;
    let (seed_store, nodes) = test_session_store().await;
    let session_store: Arc<dyn talos_store::session::SessionStore> = match product_store {
        Some(store) => {
            if store
                .get_session_record("mock-session")
                .await
                .unwrap()
                .is_none()
            {
                store
                    .insert_session_record(
                        &seed_store
                            .get_session_record("mock-session")
                            .await
                            .unwrap()
                            .unwrap(),
                    )
                    .await
                    .unwrap();
            }
            store
        }
        None => seed_store,
    };
    let reload_hook = test_config_reload_hook(config.base_config.clone());
    let plugins = talos_service::PluginManagerService::new(
        config.base_config.agent.kernel.plugins.clone(),
        config.base_config.agent.kernel.skills.clone(),
    );
    let config_service =
        talos_service::ConfigService::new(config_store, plugins.execution_gate(), reload_hook);
    let execution_directory = tempfile::tempdir().unwrap();
    let worker_store = Arc::new(
        talos_store::execution::WorkerExecutionStore::open(
            &execution_directory.path().join("worker.db"),
        )
        .await
        .unwrap(),
    );
    seed_worker_fixture(
        &worker_store,
        agent_execution_store.as_ref(),
        &config.base_config,
    )
    .await;
    let harness = Arc::new(talos_agent::LocalExecutionHarness::new(
        Arc::new(HttpFixtureHarness),
        worker_store,
    ));
    let coordinator = Arc::new(TestAgentCoordinator {
        config: config.base_config.clone(),
        memory,
        plugins: plugins.clone(),
        configuration: config_service.clone(),
        harness,
        _execution_directory: execution_directory,
    });
    let app = Arc::new(TalosService::new(
        coordinator,
        nodes,
        session_store,
        agent_execution_store,
        project_store,
        Arc::new(EmptyProfileStore),
        files,
        event_service.clone(),
        config.base_config.clone(),
        auth_service.clone(),
        user_service,
    ));
    Arc::new(HttpApiState::from_service(
        app,
        event_service,
        config.base_config,
        auth_service,
        config.auth_disabled,
        config.lifecycle,
    ))
}

async fn test_state_with_event_store_and_agent_execution_store(
    auth_disabled: bool,
    event_store: Arc<dyn AgentExecutionStore>,
    agent_execution_store: Arc<talos_store::session::DatabaseSessionStore>,
) -> Arc<HttpApiState> {
    build_test_state_with_stores(
        TestStateConfig {
            event_store,
            base_config: test_config(),
            auth_disabled,
            lifecycle: Arc::new(ServerLifecycle::new()),
            config_store: None,
        },
        agent_execution_store.clone(),
        Arc::new(MockProjectStore::new()),
        Some(agent_execution_store),
    )
    .await
}

async fn test_state(auth_disabled: bool) -> Arc<HttpApiState> {
    test_state_with_lifecycle(auth_disabled, Arc::new(ServerLifecycle::new())).await
}

async fn test_state_with_config(
    config: talos_types::TalosConfig,
    auth_disabled: bool,
) -> Arc<HttpApiState> {
    let event_store = Arc::new(RecordingAgentExecutionStore::default());
    build_test_state(TestStateConfig {
        event_store,
        base_config: config,
        auth_disabled,
        lifecycle: Arc::new(ServerLifecycle::new()),
        config_store: None,
    })
    .await
}

fn event_service_from_store(store: Arc<dyn AgentExecutionStore>) -> Arc<TimelineEventService> {
    Arc::new(TimelineEventService::new(store))
}

async fn test_state_with_event_store(
    auth_disabled: bool,
    store: Arc<dyn AgentExecutionStore>,
) -> Arc<HttpApiState> {
    test_state_with_gateway_event_store_config(auth_disabled, store, test_config()).await
}

async fn test_state_with_gateway_event_store_config(
    auth_disabled: bool,
    store: Arc<dyn AgentExecutionStore>,
    config: talos_types::TalosConfig,
) -> Arc<HttpApiState> {
    let config_store: Arc<dyn ConfigStore> =
        Arc::new(InMemoryLlmConfigStore::new(config.llm.clone()));
    test_state_with_gateway_event_store_config_and_lifecycle(
        auth_disabled,
        store,
        config,
        Arc::new(ServerLifecycle::new()),
        config_store,
    )
    .await
}

struct TestAgentCoordinator {
    config: talos_types::TalosConfig,
    memory: talos_service::MemoryService,
    plugins: talos_service::PluginManagerService,
    configuration: talos_service::ConfigService,
    harness: Arc<talos_agent::LocalExecutionHarness>,
    _execution_directory: tempfile::TempDir,
}

impl TestAgentCoordinator {
    async fn new(
        config: talos_types::TalosConfig,
        memory: talos_service::MemoryService,
        plugins: talos_service::PluginManagerService,
        configuration: talos_service::ConfigService,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(
            talos_store::execution::WorkerExecutionStore::open(
                &directory.path().join("execution.db"),
            )
            .await
            .unwrap(),
        );
        Self {
            config,
            memory,
            plugins,
            configuration,
            harness: Arc::new(talos_agent::LocalExecutionHarness::new(
                Arc::new(HttpFixtureHarness),
                store,
            )),
            _execution_directory: directory,
        }
    }
}

#[async_trait::async_trait]
impl AgentCoordinator for TestAgentCoordinator {
    async fn worker_environment(
        &self,
        _node_id: &str,
        _user_id: &str,
    ) -> TalosResult<Arc<dyn talos_coordinator::WorkerEnvironment>> {
        Ok(Arc::new(
            talos_service::WorkerEnvironmentService::new(
                Arc::new(std::sync::RwLock::new(self.config.clone())),
                Some(std::env::temp_dir()),
            )
            .with_config_service(self.configuration.clone())
            .with_memory_service(self.memory.clone())
            .with_plugin_manager(self.plugins.clone()),
        ))
    }

    async fn resolve_agent(&self, params: AgentSelectionParams) -> TalosResult<String> {
        let _ = params;
        Ok("default".to_string())
    }

    async fn ensure_agent(&self, _agent_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn select_execution_node(
        &self,
        _agent_id: &str,
        _user_id: &str,
        preferred_node_id: Option<&str>,
    ) -> TalosResult<String> {
        Ok(preferred_node_id.unwrap_or("embedded").to_string())
    }

    async fn start(&self, params: AgentRunParams) -> TalosResult<AgentRunExecution> {
        talos_agent::AgentHarness::start(self.harness.as_ref(), params).await
    }

    async fn submit_input(&self, params: AgentInputParams) -> TalosResult<AgentAdmission> {
        talos_agent::AgentHarness::submit_input(self.harness.as_ref(), params).await
    }

    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome> {
        talos_agent::AgentHarness::interrupt(self.harness.as_ref(), params).await
    }

    async fn compact_session(&self, params: AgentContext) -> TalosResult<AgentRunExecution> {
        talos_agent::AgentHarness::compact_session(self.harness.as_ref(), params).await
    }
    async fn read_session_events(
        &self,
        params: agent_protocol::AgentSessionEventsParams,
    ) -> TalosResult<agent_protocol::AgentSessionEventsResult> {
        talos_agent::AgentHarness::read_session_events(self.harness.as_ref(), params).await
    }
    async fn read_session_execution_state(
        &self,
        params: AgentSessionContext,
    ) -> TalosResult<Option<agent_protocol::SessionExecutionState>> {
        talos_agent::AgentHarness::read_session_execution_state(self.harness.as_ref(), params).await
    }
    async fn retire_session(
        &self,
        params: AgentSessionContext,
        expected: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        talos_agent::AgentHarness::retire_session(self.harness.as_ref(), params, expected).await
    }
    async fn clear_session(
        &self,
        params: AgentSessionContext,
        _expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        let _ = params;
        Ok(())
    }

    async fn close_session(
        &self,
        params: AgentSessionContext,
        _expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        let _ = params;
        Ok(())
    }

    async fn shutdown(&self, _timeout: std::time::Duration) -> TalosResult<()> {
        Ok(())
    }
}

async fn test_state_with_gateway_event_store_config_and_lifecycle(
    auth_disabled: bool,
    store: Arc<dyn AgentExecutionStore>,
    config: talos_types::TalosConfig,
    lifecycle: Arc<ServerLifecycle>,
    config_store: Arc<dyn ConfigStore>,
) -> Arc<HttpApiState> {
    build_test_state(TestStateConfig {
        event_store: store,
        base_config: config,
        auth_disabled,
        lifecycle,
        config_store: Some(config_store),
    })
    .await
}

#[derive(Default)]
struct FakeAuthService;

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
            r#type: talos_auth::UserType::Free,
            disabled: false,
            created_at: 1,
            updated_at: 1,
        }
    }
}

#[async_trait]
impl AuthService for FakeAuthService {
    async fn logout(&self, _actor: &RequestIdentity) -> TalosResult<()> {
        Ok(())
    }

    async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity> {
        self.local_identity(token)
    }

    fn local_identity(&self, token: &str) -> TalosResult<RequestIdentity> {
        if token == "reject" {
            return Err(TalosError::new(
                talos_error::ErrorCode::Unauthenticated,
                "invalid token".to_string(),
            ));
        }
        Ok(RequestIdentity {
            user_id: token.to_string(),
            username: None,
            role: talos_auth::UserRole::User,
            user_type: Default::default(),
            auth_disabled: false,
            access_token: Some(token.to_string()),
        })
    }
}

#[async_trait]
impl UserService for FakeAuthService {
    async fn get_current_user(&self, actor: &RequestIdentity) -> TalosResult<UserDto> {
        Ok(Self::user_from_actor(actor))
    }
}

fn test_auth_services() -> (Arc<dyn AuthService>, Arc<dyn UserService>) {
    let service = Arc::new(FakeAuthService);
    let auth_service: Arc<dyn AuthService> = service.clone();
    let user_service: Arc<dyn UserService> = service;
    (auth_service, user_service)
}

fn test_auth() -> RequestIdentity {
    RequestIdentity {
        user_id: "static-token".to_string(),
        username: None,
        role: talos_auth::UserRole::User,
        user_type: Default::default(),
        auth_disabled: false,
        access_token: None,
    }
}

#[path = "tests/active_turn.rs"]
mod active_turn;
#[path = "tests/auth_lifecycle.rs"]
mod auth_lifecycle;
#[path = "tests/computers.rs"]
mod computers;
#[path = "tests/events.rs"]
mod events;
#[path = "tests/files.rs"]
mod files;
#[path = "tests/handlers.rs"]
mod handlers;
#[path = "tests/message_queue.rs"]
mod message_queue;
#[path = "tests/messages.rs"]
mod messages;

#[path = "tests/automation.rs"]
mod automation;

#[path = "tests/product_routing.rs"]
mod product_routing_tests;

struct HttpFixtureHarness;
impl talos_agent::AgentHarness for HttpFixtureHarness {
    fn start(
        &self,
        params: AgentRunParams,
    ) -> talos_agent::AgentFuture<'_, TalosResult<AgentRunExecution>> {
        Box::pin(mock_agent_execution(params))
    }
    fn submit_input(
        &self,
        _params: AgentInputParams,
    ) -> talos_agent::AgentFuture<'_, TalosResult<AgentAdmission>> {
        Box::pin(async { unreachable!("Fixture has no live input") })
    }
    fn interrupt(
        &self,
        _params: AgentInterruptParams,
    ) -> talos_agent::AgentFuture<'_, TalosResult<AgentInterruptOutcome>> {
        Box::pin(async { Ok(AgentInterruptOutcome::Cancelled) })
    }
    fn compact_session(
        &self,
        params: AgentContext,
    ) -> talos_agent::AgentFuture<'_, TalosResult<AgentRunExecution>> {
        Box::pin(async move {
            let identity = params.identity();
            let started = CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: 1,
                occurred_at: 1,
                payload: CanonicalAgentEventPayload::TurnStarted(AgentTurnStarted {}),
            };
            let terminal = CanonicalAgentEvent {
                identity,
                sequence: 2,
                occurred_at: 2,
                payload: CanonicalAgentEventPayload::TurnCompleted(AgentTurnCompleted {
                    stop_reason: talos_agent_api::AgentStopReason::EndTurn,
                }),
            };
            let (progress_tx, progress_rx) = tokio::sync::mpsc::channel(1);
            drop(progress_tx);
            let (settlement_tx, settlement_rx) = tokio::sync::oneshot::channel();
            Ok(AgentRunExecution {
                admission: AgentAdmission {
                    events: vec![started],
                    permit: AgentAdmissionPermit::new(async move {
                        settlement_tx
                            .send(Ok(AgentRunSettlement::Finished {
                                result: Ok(()),
                                event: terminal,
                                metrics: AgentRunMetrics::default(),
                            }))
                            .map_err(|_| {
                                talos_error::TalosError::new(
                                    talos_error::ErrorCode::InternalError,
                                    "test receiver",
                                )
                            })?;
                        Ok(())
                    }),
                },
                events: AgentProgressEventReceiver::new(progress_rx),
                settlement: AgentRunSettlementReceiver::new(settlement_rx),
            })
        })
    }
    fn clear_session(
        &self,
        _params: AgentSessionContext,
        _expected: talos_types::SessionHistoryCursor,
    ) -> talos_agent::AgentFuture<'_, TalosResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn close_session(
        &self,
        _params: AgentSessionContext,
        _expected: talos_types::SessionHistoryCursor,
    ) -> talos_agent::AgentFuture<'_, TalosResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn shutdown(
        &self,
        _timeout: std::time::Duration,
    ) -> talos_agent::AgentFuture<'_, TalosResult<()>> {
        Box::pin(async { Ok(()) })
    }
}

async fn seed_worker_fixture(
    worker: &talos_store::execution::WorkerExecutionStore,
    product: &dyn AgentExecutionStore,
    config: &talos_types::TalosConfig,
) {
    let Some(active) = product.get_agent_active_turn("mock-session").await.unwrap() else {
        return;
    };
    let records = product
        .list_agent_turn_events(&active.identity, 1000)
        .await
        .unwrap();
    let events: Vec<CanonicalAgentEvent> = records
        .into_iter()
        .map(|record| CanonicalAgentEvent {
            identity: record.identity,
            sequence: record.sequence,
            occurred_at: record.occurred_at,
            payload: record.payload,
        })
        .collect();
    let Some((admission_end, input)) =
        events
            .iter()
            .enumerate()
            .find_map(|(index, event)| match &event.payload {
                CanonicalAgentEventPayload::MessageCompleted(message)
                    if message.role == talos_agent_api::AgentMessageRole::User =>
                {
                    Some((index, message.clone()))
                }
                _ => None,
            })
    else {
        return;
    };
    let session = AgentSessionContext {
        agent_id: "default".into(),
        session_id: "mock-session".into(),
        workspace: config.resolve_workspace("default", "static-token").unwrap(),
    };
    let context = agent_protocol::AgentContext {
        tree_identity: talos_types::SessionTreeIdentity::Primary,
        history_cursor: None,
        session: session.clone(),
        turn_id: active.identity.turn_id.clone(),
        project_id: None,
        working_directory: None,
        environment: None,
        model: test_model_info(),
        model_selection: talos_types::ModelSelection::without_reasoning(talos_types::ModelRef {
            provider: "bailian".into(),
            id: "qwen-plus".into(),
        }),
        permission_mode: Default::default(),
    };
    let command = agent_protocol::AgentExecutionCommand::Run {
        params: Box::new(AgentRunParams {
            subagent_limit: None,
            command: None,
            automation_origin: None,
            context,
            input,
            checkpoint: None,
            continuation: None,
        }),
    };
    let mut reducer = talos_agent::AgentEventReducer::new(active.identity.clone());
    for event in &events[..=admission_end] {
        reducer.apply(event.clone()).unwrap();
    }
    worker
        .admit(
            &session,
            &command,
            events[..=admission_end].to_vec(),
            &reducer.checkpoint(),
        )
        .await
        .unwrap();
    if admission_end + 1 < events.len() {
        for event in &events[admission_end + 1..] {
            reducer.apply(event.clone()).unwrap();
        }
        let binding = product
            .get_agent_continuation_binding(&active.identity)
            .await
            .unwrap();
        let settlement =
            binding.map(
                |binding| agent_protocol::AgentExecutionSettlement::WaitingForInput {
                    continuation: binding.provider_ref,
                    metrics: reducer.checkpoint().metrics.clone(),
                },
            );
        worker
            .append(
                &session,
                events[admission_end + 1..].to_vec(),
                &reducer.checkpoint(),
                settlement,
            )
            .await
            .unwrap();
    }
}
