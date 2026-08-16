use super::*;
use async_trait::async_trait;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use talos_auth::{AuthService, RequestIdentity, UserDto, UserService};
use talos_config::{ConfigStore, InMemoryLlmConfigStore};
use talos_coordinator::SessionRuntime;
use talos_error::{TalosError, TalosResult};
use talos_service::{TalosService, TurnExecutor};
use talos_service::{TimelineEventRetentionConfig, TimelineEventService};
use talos_store::profile::{
    ProfileDailyUsageSummary, ProfileModelUsageSummary, ProfileStore, ProfileUsageSummary,
    TurnUsageUpdate,
};
use talos_store::project::MockProjectStore;
use talos_store::session::{
    ActiveTurnRecord, ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, IdempotencyRecord,
    IdempotencyReservation, IdempotencyStatus, MessageCommandResult, MessageIdempotencyStore,
    SessionPatchAck, SessionQuery, SessionRecord, SessionStore, SessionSummary, TimelineEventId,
    TimelineEventStore, UserActionRecord, UserActionStore,
};
use talos_types::talos::{
    AgentEvent, AgentEventPayload, AgentEventType, StopReason, TalosContext, TalosItemDeltaEvent,
    TalosItemStartedEvent, TalosTimelineContent, TalosTimelineDelta, TalosTimelineItem,
    TalosTimelineItemKind, TalosTimelineItemStatus, TalosTimelinePhase, TalosToolProgressKind,
    TalosTurnCompletedEvent, TalosTurnStartedEvent, TokenUsage, TurnExecutionMetrics,
};
use talos_types::{
    DynamicToolResultParams, ModelInfo, PendingUserAction, SessionListRequest, TalosSession,
    UserActionParams, UserActionState,
};
use tower::util::ServiceExt;

use crate::server::events::{get_active_turn_handler, subscribe_timeline_events_handler};
use crate::server::messages::{
    interrupt_turn_handler, retry_turn_handler, start_turn_handler,
    submit_dynamic_tool_result_handler,
};
use crate::server::misc::{get_config_handler, get_llm_handler, put_config_handler};

#[path = "tests/support.rs"]
mod support;
use support::*;

fn expect_success<T>(response: talos_types::ApiResponse<T>) -> T {
    match response {
        talos_types::ApiResponse::Success { data } => data,
        talos_types::ApiResponse::Error { error } => {
            panic!("expected success response, got error: {error:?}")
        }
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
                "message": "Invalid request"
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
    async fn record_turn_usage(&self, _update: TurnUsageUpdate) -> TalosResult<()> {
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

#[async_trait]
trait SessionService: Send + Sync {
    async fn resolve_target_agent(&self, _context: &TalosContext) -> TalosResult<String> {
        Ok("default".to_string())
    }

    async fn list_sessions(&self, request: SessionListRequest) -> TalosResult<Vec<TalosSession>>;

    async fn count_sessions(&self, request: SessionListRequest) -> TalosResult<usize>;

    async fn get_session(&self, session_id: &str) -> TalosResult<Option<TalosSession>>;

    async fn clear_session(&self, session_id: &str) -> TalosResult<TalosSession>;

    async fn close_session(&self, session_id: &str) -> TalosResult<()>;

    async fn delete_session(&self, session_id: &str) -> TalosResult<SessionPatchAck>;

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()>;
}

fn test_model_info() -> ModelInfo {
    ModelInfo::from_provider_model(
        "bailian",
        talos_types::ProviderModelConfig {
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
    config.agent.kernel.workspace_dir = std::env::temp_dir()
        .join(format!("talos-http-test-{}", uuid::Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    config.llm.provider = "bailian".to_string();
    config.llm.model = "qwen-plus".to_string();
    config.llm.bailian.models = vec![test_model_info().model];
    config
}

fn isolated_test_config() -> (talos_types::TalosConfig, std::path::PathBuf) {
    let config = test_config();
    let workspace_dir = std::path::PathBuf::from(&config.agent.kernel.workspace_dir);
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should exist");
    (config, workspace_dir)
}

async fn test_state_with_lifecycle(
    auth_disabled: bool,
    lifecycle: Arc<ServerLifecycle>,
) -> Arc<HttpApiState> {
    let event_store = Arc::new(RecordingEventStore::default());
    build_test_state(TestStateConfig {
        turn_executor: Arc::new(MockGateway),
        session_service: Arc::new(MockSessionService::default()),
        event_store,
        base_config: test_config(),
        auth_disabled,
        lifecycle,
        config_store: None,
    })
    .await
}

struct TestStateConfig {
    turn_executor: Arc<dyn TurnExecutor>,
    session_service: Arc<dyn SessionService>,
    event_store: Arc<RecordingEventStore>,
    base_config: talos_types::TalosConfig,
    auth_disabled: bool,
    lifecycle: Arc<ServerLifecycle>,
    config_store: Option<Arc<dyn ConfigStore>>,
}

async fn build_test_state(config: TestStateConfig) -> Arc<HttpApiState> {
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
    let project_store = Arc::new(MockProjectStore::new());
    let files = test_file_service(
        project_store.clone(),
        config.base_config.storage.files.clone(),
    )
    .await;
    let app = Arc::new(TalosService::new_with_session_store(
        config.turn_executor,
        Arc::new(SessionServiceRuntimeAdapter::new(
            config.session_service.clone(),
        )),
        Arc::new(SessionStoreAdapter::new(config.session_service)),
        Arc::new(talos_store::session::FileSessionArtifactStore::default()),
        project_store,
        Arc::new(EmptyProfileStore),
        files,
        event_service.clone(),
        config.event_store,
        config.base_config.clone(),
        memory,
        config_store,
        vec![test_model_info()],
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

async fn test_state(auth_disabled: bool) -> Arc<HttpApiState> {
    test_state_with_lifecycle(auth_disabled, Arc::new(ServerLifecycle::new())).await
}

async fn test_state_with_config(
    config: talos_types::TalosConfig,
    auth_disabled: bool,
) -> Arc<HttpApiState> {
    let event_store = Arc::new(RecordingEventStore::default());
    build_test_state(TestStateConfig {
        turn_executor: Arc::new(MockGateway),
        session_service: Arc::new(MockSessionService::default()),
        event_store,
        base_config: config,
        auth_disabled,
        lifecycle: Arc::new(ServerLifecycle::new()),
        config_store: None,
    })
    .await
}

fn event_service_from_store(store: Arc<RecordingEventStore>) -> Arc<TimelineEventService> {
    let event_store: Arc<dyn TimelineEventStore> = store.clone();
    let active_turn_store: Arc<dyn ActiveTurnStore> = store.clone();
    Arc::new(TimelineEventService::new(
        event_store,
        active_turn_store,
        store as Arc<dyn talos_store::session::UserActionStore>,
        TimelineEventRetentionConfig::default(),
    ))
}

async fn test_state_with_event_store(
    auth_disabled: bool,
    store: Arc<RecordingEventStore>,
) -> Arc<HttpApiState> {
    test_state_with_gateway_event_store_config(
        Arc::new(MockGateway),
        auth_disabled,
        store,
        test_config(),
    )
    .await
}

async fn test_state_with_gateway_event_store_config(
    turn_executor: Arc<dyn TurnExecutor>,
    auth_disabled: bool,
    store: Arc<RecordingEventStore>,
    config: talos_types::TalosConfig,
) -> Arc<HttpApiState> {
    let config_store: Arc<dyn ConfigStore> =
        Arc::new(InMemoryLlmConfigStore::new(config.llm.clone()));
    test_state_with_gateway_event_store_config_and_lifecycle(
        turn_executor,
        auth_disabled,
        store,
        config,
        Arc::new(ServerLifecycle::new()),
        config_store,
    )
    .await
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

    async fn compact_session(
        &self,
        _params: talos_coordinator::AgentSessionCompactionParams,
    ) -> talos_error::TalosResult<talos_types::TokenUsage> {
        Ok(talos_types::TokenUsage::default())
    }

    async fn clear_session(&self, session: TalosSession) -> talos_error::TalosResult<()> {
        self.service
            .clear_session(&session.context.session_id)
            .await
            .map(|_| ())
    }

    async fn close_session(&self, session: TalosSession) -> talos_error::TalosResult<()> {
        self.service
            .close_session(&session.context.session_id)
            .await
    }

    async fn prepare_delete_session(&self, _session: TalosSession) -> talos_error::TalosResult<()> {
        Ok(())
    }

    async fn cancel_active_turn(
        &self,
        params: talos_types::CancelActiveTurnParams,
    ) -> talos_error::TalosResult<()> {
        self.service
            .cancel_active_turn(&params.session.context.session_id)
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
        Ok(SessionRecord::from_session(&session, "default"))
    }
}

#[async_trait::async_trait]
impl SessionStore for SessionStoreAdapter {
    async fn insert_session_record(
        &self,
        _record: &SessionRecord,
        _scope: &str,
    ) -> talos_error::TalosResult<()> {
        Ok(())
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

    async fn get_subagent_record(
        &self,
        _tree_root_session_id: &str,
        _agent_path: &talos_types::AgentPath,
    ) -> talos_error::TalosResult<Option<SessionRecord>> {
        Ok(None)
    }

    async fn list_session_records(
        &self,
        query: &SessionQuery,
    ) -> talos_error::TalosResult<Vec<SessionRecord>> {
        let mut records = Vec::new();
        for session in self
            .service
            .list_sessions(SessionListRequest {
                project_id: query.project_id.clone(),
                limit: query.limit,
                offset: query.offset,
            })
            .await?
        {
            records.push(self.to_record(session)?);
        }
        Ok(records)
    }

    async fn count_session_records(&self, query: &SessionQuery) -> talos_error::TalosResult<usize> {
        self.service
            .count_sessions(SessionListRequest {
                project_id: query.project_id.clone(),
                limit: query.limit,
                offset: query.offset,
            })
            .await
    }

    async fn summarize_sessions(&self, _user_id: &str) -> talos_error::TalosResult<SessionSummary> {
        Ok(SessionSummary::default())
    }

    async fn has_active_subagent_turn(
        &self,
        _tree_root_session_id: &str,
    ) -> talos_error::TalosResult<bool> {
        Ok(false)
    }

    async fn update_session_record(
        &self,
        _record: &SessionRecord,
    ) -> talos_error::TalosResult<bool> {
        Ok(true)
    }

    async fn advance_memory_processed_until_seq(
        &self,
        _session_id: &str,
        _expected_until_seq: u64,
        _next_until_seq: u64,
    ) -> talos_error::TalosResult<()> {
        Ok(())
    }

    async fn delete_session_record(&self, session_id: &str) -> talos_error::TalosResult<bool> {
        Ok(self.service.delete_session(session_id).await?.success)
    }

    async fn delete_session_tree(
        &self,
        tree_root_session_id: &str,
    ) -> talos_error::TalosResult<bool> {
        Ok(self
            .service
            .delete_session(tree_root_session_id)
            .await?
            .success)
    }
}

async fn test_state_with_gateway_event_store_config_and_lifecycle(
    turn_executor: Arc<dyn TurnExecutor>,
    auth_disabled: bool,
    store: Arc<RecordingEventStore>,
    config: talos_types::TalosConfig,
    lifecycle: Arc<ServerLifecycle>,
    config_store: Arc<dyn ConfigStore>,
) -> Arc<HttpApiState> {
    build_test_state(TestStateConfig {
        turn_executor,
        session_service: Arc::new(MockSessionService::default()),
        event_store: store,
        base_config: config,
        auth_disabled,
        lifecycle,
        config_store: Some(config_store),
    })
    .await
}

#[derive(Default)]
struct RecordingEventStore {
    events: std::sync::Mutex<Vec<AgentEvent>>,
    active_turns: std::sync::Mutex<std::collections::HashMap<String, ActiveTurnRecord>>,
    idempotency_records:
        std::sync::Mutex<std::collections::HashMap<(String, String), IdempotencyRecord>>,
    user_actions:
        std::sync::Mutex<std::collections::HashMap<(String, String, String), UserActionRecord>>,
}

fn pending_dynamic_record(
    session_id: &str,
    turn_id: &str,
    action_id: &str,
    item_id: Option<&str>,
    call_id: &str,
    tool_name: &str,
) -> UserActionRecord {
    UserActionRecord {
        session_id: session_id.to_string(),
        turn_id: turn_id.to_string(),
        action: PendingUserAction {
            action_id: action_id.to_string(),
            call_id: call_id.to_string(),
            item_id: item_id.map(str::to_string),
            state: UserActionState::Pending,
            params: UserActionParams::DynamicToolResult(DynamicToolResultParams {
                tool_name: tool_name.to_string(),
            }),
        },
        response: None,
        version: 0,
        updated_at: 125,
    }
}

async fn record_pending_tool_events(
    store: &RecordingEventStore,
    session_id: &str,
    turn_id: &str,
    message_id: &str,
    item_id: &str,
    call_id: &str,
    tool_name: &str,
) {
    let base_event = |sequence, event_type, payload| AgentEvent {
        event_seq: None,
        event_id: None,
        created_at_ms: None,
        event_type,
        turn_id: turn_id.to_string(),
        message_id: message_id.to_string(),
        session_id: session_id.to_string(),
        sequence,
        payload,
    };
    store
        .append_event(base_event(
            0,
            AgentEventType::ItemStarted,
            AgentEventPayload::ItemStarted(TalosItemStartedEvent {
                request_id: "request-dynamic".to_string(),
                turn_id: turn_id.to_string(),
                item: TalosTimelineItem {
                    item_id: item_id.to_string(),
                    parent_item_id: None,
                    kind: TalosTimelineItemKind::ToolCall,
                    phase: TalosTimelinePhase::Acting,
                    status: TalosTimelineItemStatus::Running,
                    title: None,
                    started_at_ms: 1,
                    completed_at_ms: None,
                    content: TalosTimelineContent::ToolCall {
                        call_id: call_id.to_string(),
                        name: tool_name.to_string(),
                        arguments: "{}".to_string(),
                        progress: Vec::new(),
                        result: None,
                    },
                },
            }),
        ))
        .await
        .expect("pending tool start event should persist");
    store
        .append_event(base_event(
            1,
            AgentEventType::ItemDelta,
            AgentEventPayload::ItemDelta(TalosItemDeltaEvent {
                request_id: "request-dynamic".to_string(),
                turn_id: turn_id.to_string(),
                item_id: item_id.to_string(),
                delta: TalosTimelineDelta::Status {
                    status: TalosTimelineItemStatus::WaitingUserAction,
                },
            }),
        ))
        .await
        .expect("pending tool status event should persist");
    store
        .append_event(base_event(
            2,
            AgentEventType::TurnCompleted,
            AgentEventPayload::TurnCompleted(TalosTurnCompletedEvent {
                request_id: "request-dynamic".to_string(),
                turn_id: turn_id.to_string(),
                completed_at_ms: 1,
                stop_reason: StopReason::WaitingUserAction,
                usage: TokenUsage::default(),
                metrics: Some(TurnExecutionMetrics {
                    model_rounds: 1,
                    tool_calls: 1,
                    stop_reason: StopReason::WaitingUserAction,
                    ..TurnExecutionMetrics::default()
                }),
                artifact_seq: Some(1),
            }),
        ))
        .await
        .expect("pending turn metrics should persist");
}

#[async_trait]
impl UserActionStore for RecordingEventStore {
    async fn create(&self, record: UserActionRecord) -> TalosResult<UserActionRecord> {
        let key = (
            record.session_id.clone(),
            record.turn_id.clone(),
            record.action.action_id.clone(),
        );
        let mut actions = self.user_actions.lock().unwrap();
        match actions.get(&key) {
            Some(current) if current == &record => Ok(current.clone()),
            Some(_) => Err(TalosError::public(
                talos_error::ErrorCode::Conflict,
                "user action already exists",
            )),
            None => {
                actions.insert(key, record.clone());
                Ok(record)
            }
        }
    }

    async fn get(
        &self,
        session_id: &str,
        turn_id: &str,
        action_id: &str,
    ) -> TalosResult<Option<UserActionRecord>> {
        Ok(self
            .user_actions
            .lock()
            .unwrap()
            .get(&(
                session_id.to_string(),
                turn_id.to_string(),
                action_id.to_string(),
            ))
            .cloned())
    }

    async fn compare_and_set(
        &self,
        expected: &UserActionRecord,
        next: UserActionRecord,
    ) -> TalosResult<UserActionRecord> {
        let key = (
            expected.session_id.clone(),
            expected.turn_id.clone(),
            expected.action.action_id.clone(),
        );
        let mut actions = self.user_actions.lock().unwrap();
        if actions.get(&key) != Some(expected) {
            return Err(TalosError::public(
                talos_error::ErrorCode::Conflict,
                "user action state or version changed",
            ));
        }
        actions.insert(key, next.clone());
        Ok(next)
    }
}

#[async_trait::async_trait]
impl TimelineEventStore for RecordingEventStore {
    async fn append_event(&self, mut event: AgentEvent) -> TalosResult<AgentEvent> {
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
    ) -> TalosResult<Vec<AgentEvent>> {
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

    async fn list_turn_events(
        &self,
        session_id: &str,
        turn_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<AgentEvent>> {
        let mut events = self
            .events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .filter(|event| event.session_id == session_id && event.turn_id == turn_id)
            .take(limit.max(1))
            .cloned()
            .collect::<Vec<_>>();
        events.reverse();
        Ok(events)
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
                current_action_id: update.current_action_id,
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
            result: None,
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
        result: &MessageCommandResult,
        now_ms: i64,
    ) -> TalosResult<()> {
        let mut records = self.idempotency_records.lock().unwrap();
        let key = (session_id.to_string(), idempotency_key.to_string());
        if let Some(record) = records.get_mut(&key) {
            record.status = status;
            record.message_id = message_id.map(ToString::to_string);
            record.result = Some(result.clone());
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
        if token == "reject" {
            return Err(TalosError::public(
                talos_error::ErrorCode::Unauthenticated,
                "invalid token".to_string(),
            ));
        }
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
        auth_disabled: false,
        access_token: None,
    }
}

#[path = "tests/active_turn.rs"]
mod active_turn;
#[path = "tests/auth_lifecycle.rs"]
mod auth_lifecycle;
#[path = "tests/events.rs"]
mod events;
#[path = "tests/files.rs"]
mod files;
#[path = "tests/handlers.rs"]
mod handlers;
#[path = "tests/messages.rs"]
mod messages;
