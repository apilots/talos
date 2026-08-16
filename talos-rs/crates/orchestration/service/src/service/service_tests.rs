use super::*;
use crate::access::CreateSessionRequest;
use crate::test_support::run_test_migrations;
use crate::TimelineEventRetentionConfig;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::stream;
use opendal::{services::Memory, Operator};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use talos_auth::{AuthService, RequestIdentity, UserDto, UserService};
use talos_config::{resolve_effective_kernel_workspace, ConfigStore, InMemoryLlmConfigStore};
use talos_error::{TalosError, TalosResult};
use talos_storage::SqliteStorage;
use talos_store::file::DatabaseFileStore;
use talos_store::memory::MemoryStore;
use talos_store::profile::{DatabaseProfileStore, ProfileStore};
use talos_store::project::StorageProjectStore;
use talos_store::session::{
    ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, DatabaseSessionStore,
    FileSessionArtifactStore, IdempotencyReservation, MessageIdempotencyStore,
    SessionArtifactStore, SessionStore, TalosTurn, TimelineEventId, TimelineEventStore,
};
use talos_types::agent_dispatch::{AgentDispatchInputParams, AgentDispatchParams};
use talos_types::talos::{
    AgentEvent, AgentEventPayload, ContextCompactionOutcome, ContextCompactionStage,
    ContextCompactionTrigger, RunItem, StopReason, TalosContext, TalosInputItem, TalosProvider,
    TalosSourceType, TalosTimelineContent, TalosTimelineDelta, TalosTimelineItemStatus, TokenUsage,
};

enum TestTurnExecutorMode {
    Accepted,
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

struct TestAuthService;

struct FailOnceTimelineEventStore {
    inner: Arc<dyn TimelineEventStore>,
    fail_on_call: usize,
    calls: AtomicUsize,
}

#[async_trait]
impl TimelineEventStore for FailOnceTimelineEventStore {
    async fn append_event(&self, event: AgentEvent) -> TalosResult<AgentEvent> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == self.fail_on_call {
            return Err(TalosError::internal(
                "injected compaction event persistence failure".to_string(),
            ));
        }
        self.inner.append_event(event).await
    }

    async fn list_events_after(
        &self,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<Vec<AgentEvent>> {
        self.inner
            .list_events_after(session_id, after_event_id, limit)
            .await
    }

    async fn list_turn_events(
        &self,
        session_id: &str,
        turn_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<AgentEvent>> {
        self.inner
            .list_turn_events(session_id, turn_id, limit)
            .await
    }

    async fn latest_event_id(&self, session_id: &str) -> TalosResult<Option<TimelineEventId>> {
        self.inner.latest_event_id(session_id).await
    }

    async fn oldest_event_id(&self, session_id: &str) -> TalosResult<Option<TimelineEventId>> {
        self.inner.oldest_event_id(session_id).await
    }

    async fn delete_completed_events_before(
        &self,
        before_timestamp_ms: i64,
        limit: usize,
    ) -> TalosResult<u64> {
        self.inner
            .delete_completed_events_before(before_timestamp_ms, limit)
            .await
    }

    async fn delete_overflow_events_per_session(
        &self,
        max_events_per_session: usize,
        limit: usize,
    ) -> TalosResult<u64> {
        self.inner
            .delete_overflow_events_per_session(max_events_per_session, limit)
            .await
    }
}

fn configured_test_config() -> TalosConfig {
    let mut config = TalosConfig::default();
    config.llm.anthropic.models = vec![talos_config::ProviderModelConfig {
        id: config.llm.model.clone(),
        name: "Default test model".to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }];
    config
}

#[async_trait]
impl TurnExecutor for TestTurnExecutor {
    async fn validate_agent_id(&self, _agent_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn start_turn_stream(
        &self,
        request: AgentDispatchParams,
    ) -> TalosResult<tokio::sync::mpsc::Receiver<AgentEvent>> {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        match self.mode {
            TestTurnExecutorMode::Accepted => {
                tokio::spawn(async move {
                    tx.send(AgentEvent {
                        event_seq: None,
                        event_id: None,
                        created_at_ms: None,
                        event_type: talos_types::talos::AgentEventType::TurnStarted,
                        turn_id: request.turn_id.clone(),
                        message_id: request.session.context.message_id.clone(),
                        session_id: request.session.context.session_id.clone(),
                        sequence: 0,
                        payload: AgentEventPayload::TurnStarted(
                            talos_types::talos::TalosTurnStartedEvent {
                                request_id: "request-test".to_string(),
                                turn_id: request.turn_id,
                                started_at_ms: 1,
                            },
                        ),
                    })
                    .await
                    .expect("started event receiver should remain open");
                    tokio::time::sleep(Duration::from_secs(60)).await;
                });
            }
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
        params: talos_coordinator::AgentSessionCompactionParams,
    ) -> TalosResult<talos_types::TokenUsage> {
        let mut timeline = talos_agent_kernel::TimelineBuilder::new(
            params.identity.request_id,
            params.identity.turn_id,
            params.identity.message_id,
            params.session.context.session_id,
        );
        params.event_tx.send(timeline.start_turn()).await.unwrap();
        let started = timeline.start_context_compaction(
            ContextCompactionTrigger::UserRequested,
            ContextCompactionStage::Preparing,
            None,
        );
        let item_id = match &started.payload {
            AgentEventPayload::ItemStarted(payload) => payload.item.item_id.clone(),
            _ => unreachable!("timeline builder should start an item"),
        };
        params.event_tx.send(started).await.unwrap();
        let final_content = TalosTimelineContent::ContextCompaction {
            trigger: ContextCompactionTrigger::UserRequested,
            stage: ContextCompactionStage::Completed,
            outcome: Some(ContextCompactionOutcome::NoChanges),
            metrics: None,
            message: None,
        };
        params
            .event_tx
            .send(timeline.append_delta(
                &item_id,
                TalosTimelineDelta::ContextCompaction {
                    stage: Some(ContextCompactionStage::Completed),
                    outcome: Some(ContextCompactionOutcome::NoChanges),
                    metrics: None,
                    message: None,
                },
            )?)
            .await
            .unwrap();
        params
            .event_tx
            .send(timeline.complete_item(
                &item_id,
                TalosTimelineItemStatus::Completed,
                Some(final_content),
            )?)
            .await
            .unwrap();
        params
            .event_tx
            .send(timeline.complete_turn(StopReason::EndTurn, TokenUsage::default(), None, None))
            .await
            .unwrap();
        Ok(TokenUsage::default())
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
            TestCancelMode::MissingRuntimeTurn => Err(TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                "active turn for session session-interrupt-runtime-missing".to_string(),
            )),
        }
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
    test_application_with_runtime_and_event_failure(turn_executor, session_runtime, config, None)
        .await
}

async fn test_application_with_runtime_and_event_failure(
    turn_executor: Arc<dyn TurnExecutor>,
    session_runtime: Arc<dyn SessionRuntime>,
    config: TalosConfig,
    fail_on_event_call: Option<usize>,
) -> (TalosService, Arc<DatabaseSessionStore>) {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_test_migrations(storage.pool()).await;
    let profile_store: Arc<dyn ProfileStore> =
        Arc::new(DatabaseProfileStore::new(storage.pool().clone()));
    let store = Arc::new(DatabaseSessionStore::new(storage.clone()).await.unwrap());
    let file_store = Arc::new(DatabaseFileStore::new(storage.pool().clone()));
    let project_store = Arc::new(StorageProjectStore::new(storage).await.unwrap());
    let file_operator = Operator::new(Memory::default()).unwrap().finish();
    let files = Arc::new(
        crate::FileService::new(
            file_store,
            project_store.clone(),
            BTreeMap::from([("local".to_string(), file_operator)]),
            talos_config::FileStorageConfig::default(),
        )
        .unwrap(),
    );
    let event_store: Arc<dyn TimelineEventStore> = match fail_on_event_call {
        Some(fail_on_call) => Arc::new(FailOnceTimelineEventStore {
            inner: store.clone(),
            fail_on_call,
            calls: AtomicUsize::new(0),
        }),
        None => store.clone(),
    };
    let active_turn_store: Arc<dyn ActiveTurnStore> = store.clone();
    let event_service = Arc::new(TimelineEventService::new(
        event_store,
        active_turn_store,
        store.clone() as Arc<dyn talos_store::session::UserActionStore>,
        TimelineEventRetentionConfig::default(),
    ));
    let idempotency_store: Arc<dyn MessageIdempotencyStore> = store.clone();
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
    let service = TalosService::new_with_session_store(
        turn_executor,
        session_runtime,
        store.clone(),
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
        user_permission_mode: None,
        project_id: None,
        current_directory: None,
        client_request_id: None,
    }
}

#[tokio::test]
async fn first_turn_persists_selected_permission_mode() {
    let (service, _store) = test_application(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Accepted,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    let mut request = start_turn_request("session-initial-permission", "hello");
    request.user_permission_mode = Some(talos_types::UserPermissionMode::AskForApproval);

    service
        .start_turn(&actor, request, None)
        .await
        .expect("first turn should create the session");

    let session = service
        .get_session(&actor, "session-initial-permission")
        .await
        .expect("created session should be readable");
    assert_eq!(
        (
            session.conf.user_permission_mode,
            session.effective_user_permission_mode,
        ),
        (
            Some(talos_types::UserPermissionMode::AskForApproval),
            talos_types::UserPermissionMode::AskForApproval,
        )
    );
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
        user_permission_mode: None,
        workspace_id: None,
        project_id: None,
        current_directory,
    }
}

#[tokio::test]
async fn session_history_reads_artifacts_from_the_session_user_workspace() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let mut config = configured_test_config();
    config.agent.kernel.workspace_dir = workspace.path().to_string_lossy().to_string();
    let (service, _store) = test_application(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        config.clone(),
    )
    .await;
    let actor = Actor::regular("member-user");
    let session_id = "session-user-history";
    service
        .create_session(
            &actor,
            create_session_params(session_id, request_context(), None),
        )
        .await
        .expect("session should be created");
    let user_workspace = resolve_effective_kernel_workspace(&config.agent.kernel, "member-user")
        .expect("user workspace")
        .user_workspace_dir;
    FileSessionArtifactStore::default()
        .append_turn(
            &user_workspace,
            &TalosTurn {
                turn_id: "turn-history".to_string(),
                message_id: "message-history".to_string(),
                session_id: session_id.to_string(),
                seq: 1,
                timestamp: chrono::Utc::now(),
                items: vec![RunItem::UserInput {
                    text: "remember this".to_string(),
                    file_ids: Vec::new(),
                }],
                usage: TokenUsage::default(),
                stop_reason: StopReason::EndTurn,
            },
        )
        .await
        .expect("turn artifact should be written");

    let history = service
        .get_session_history(&actor, session_id)
        .await
        .expect("session history should be readable");

    assert_eq!(history.messages.len(), 1);
    assert_eq!(history.messages[0].message_id, "message-history");
}

#[tokio::test]
async fn existing_session_start_turn_applies_model_selection_before_dispatch() {
    let mut config = configured_test_config();
    config
        .llm
        .anthropic
        .models
        .push(talos_config::ProviderModelConfig {
            id: "turn-model".to_string(),
            name: "Turn Model".to_string(),
            context_window: 128_000,
            reasoning: true,
            reasoning_effort: talos_config::ReasoningEffort::Medium,
            speed: talos_config::ModelSpeed::Standard,
            input: vec![talos_config::ModelInputKind::Text],
            cost: talos_config::ModelCostConfig::default(),
        });
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
    request.model_selection = Some(talos_types::ModelSelection::without_reasoning(
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

#[tokio::test]
async fn existing_session_start_turn_does_not_change_permission_mode() {
    let (service, _store) = test_application(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-existing-permission", request_context(), None),
        )
        .await
        .expect("session should be created");

    let mut request = start_turn_request("session-existing-permission", "hello");
    request.user_permission_mode = Some(talos_types::UserPermissionMode::AskForApproval);
    let _ = service.start_turn(&actor, request, None).await;

    let session = service
        .get_session(&actor, "session-existing-permission")
        .await
        .expect("session should remain readable");
    assert_eq!(
        (
            session.conf.user_permission_mode,
            session.effective_user_permission_mode,
        ),
        (None, talos_types::UserPermissionMode::FullAccess)
    );
}

#[tokio::test]
async fn explicit_compaction_persists_the_existing_timeline_events() {
    let (service, _store) = test_application(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-compact-events", request_context(), None),
        )
        .await
        .expect("session should be created");

    let session = service
        .compact_session(&actor, "session-compact-events")
        .await
        .expect("compaction should complete");
    assert_eq!(session.state.message_count, 0);

    let recovery = service
        .events
        .recover_timeline("session-compact-events", None, 16)
        .await
        .expect("compaction timeline should be recoverable");
    let events = recovery.replay.events;
    assert_eq!(events.len(), 5);
    assert_eq!(
        events
            .iter()
            .map(|event| event.event.event_type)
            .collect::<Vec<_>>(),
        vec![
            talos_types::talos::AgentEventType::TurnStarted,
            talos_types::talos::AgentEventType::ItemStarted,
            talos_types::talos::AgentEventType::ItemDelta,
            talos_types::talos::AgentEventType::ItemCompleted,
            talos_types::talos::AgentEventType::TurnCompleted,
        ]
    );
    assert!(events.iter().all(|event| {
        event.session_id == "session-compact-events"
            && event.turn_id.starts_with("turn_")
            && event.message_id.starts_with("msg_")
            && event.turn_id.strip_prefix("turn_") == event.message_id.strip_prefix("msg_")
    }));
    let request_id = match &events[0].event.payload {
        AgentEventPayload::TurnStarted(payload) => payload.request_id.as_str(),
        _ => unreachable!("first compaction event should start the maintenance turn"),
    };
    assert_eq!(
        request_id.strip_prefix("req_"),
        events[0].turn_id.strip_prefix("turn_")
    );
    assert_eq!(
        recovery.active_turn.status,
        talos_types::TimelineActiveTurnStatus::Completed
    );
}

#[tokio::test]
async fn explicit_compaction_records_terminal_after_an_intermediate_event_failure() {
    let runtime = Arc::new(TestSessionRuntime {
        cancel_mode: TestCancelMode::Ok,
        cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
    });
    let (service, store) = test_application_with_runtime_and_event_failure(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        runtime,
        configured_test_config(),
        Some(2),
    )
    .await;
    let actor = Actor::regular("member-user");
    let session_id = "session-compact-event-failure";
    service
        .create_session(
            &actor,
            create_session_params(session_id, request_context(), None),
        )
        .await
        .expect("session should be created");

    let error = service
        .compact_session(&actor, session_id)
        .await
        .expect_err("the first persistence error should be returned");

    assert!(error.is_internal());
    assert_eq!(
        error.diagnostic_message(),
        "injected compaction event persistence failure"
    );
    assert_eq!(
        store
            .get_active_turn(session_id)
            .await
            .expect("active turn read")
            .expect("compaction active turn")
            .status,
        ActiveTurnStatus::Completed
    );
    let events = store
        .list_events_after(session_id, None, 16)
        .await
        .expect("persisted compaction events");
    assert_eq!(
        events.last().map(|event| event.event_type),
        Some(talos_types::talos::AgentEventType::TurnCompleted)
    );
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
    let mut config = configured_test_config();
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
        configured_test_config(),
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
        configured_test_config(),
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

    assert!(result
        .as_ref()
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::IdempotencyConflict)));
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
        configured_test_config(),
    )
    .await;
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "session-active-conflict".to_string(),
            turn_id: "turn-existing".to_string(),
            message_id: None,
            status: ActiveTurnStatus::Running,
            last_event_id: None,
            current_action_id: None,
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
async fn patch_session_updates_permission_mode_when_idle() {
    let (service, _store) = test_application(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-patch-permission", request_context(), None),
        )
        .await
        .expect("session should be created");

    let ack = service
        .patch_session(
            &actor,
            "session-patch-permission",
            SessionPatch {
                conf: Some(talos_types::SessionConf {
                    user_permission_mode: Some(talos_types::UserPermissionMode::AskForApproval),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .expect("idle session patch should succeed");

    assert!(ack.success);
    let session = service
        .get_session(&actor, "session-patch-permission")
        .await
        .expect("patched session should be readable");
    assert_eq!(
        session.conf.user_permission_mode,
        Some(talos_types::UserPermissionMode::AskForApproval)
    );
    assert_eq!(
        session.effective_user_permission_mode,
        talos_types::UserPermissionMode::AskForApproval
    );
}

#[tokio::test]
async fn patch_session_rejects_running_active_turn() {
    let (service, store) = test_application(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    service
        .create_session(
            &actor,
            create_session_params("session-patch-active", request_context(), None),
        )
        .await
        .expect("session should be created");
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "session-patch-active".to_string(),
            turn_id: "turn-existing".to_string(),
            message_id: None,
            status: ActiveTurnStatus::Running,
            last_event_id: None,
            current_action_id: None,
            updated_at: 1,
        })
        .await
        .expect("active turn insert should succeed");

    let error = service
        .patch_session(&actor, "session-patch-active", SessionPatch::default())
        .await
        .expect_err("active session patch should be rejected");

    assert!(error.is_code(talos_error::ErrorCode::ActiveTurnRunning));
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
        configured_test_config(),
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
        configured_test_config(),
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

    assert!(error.is_code(talos_error::ErrorCode::ResourceNotFound));
}

#[tokio::test]
async fn interrupt_turn_finalizes_orphaned_active_turn() {
    let cancelled_turn_ids = Arc::new(Mutex::new(Vec::new()));
    let (service, store) = test_application_with_runtime(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        Arc::new(TestSessionRuntime {
            cancel_mode: TestCancelMode::MissingRuntimeTurn,
            cancelled_turn_ids: cancelled_turn_ids.clone(),
        }),
        configured_test_config(),
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
            current_action_id: None,
            updated_at: 1,
        })
        .await
        .expect("active turn should be inserted");

    let response = service
        .interrupt_turn(
            &actor,
            talos_types::InterruptTurnRequest {
                session_id: "session-interrupt-runtime-missing".to_string(),
            },
        )
        .await
        .expect("orphaned active turn should be finalized");

    assert!(response.success);
    assert_eq!(
        *cancelled_turn_ids.lock().unwrap(),
        vec!["turn-existing".to_string()]
    );
    let active_turn = store
        .get_active_turn("session-interrupt-runtime-missing")
        .await
        .expect("active turn should load")
        .expect("active turn should remain as terminal recovery state");
    assert_eq!(active_turn.status, ActiveTurnStatus::Failed);
    let events = store
        .list_turn_events("session-interrupt-runtime-missing", "turn-existing", 10)
        .await
        .expect("turn events should load");
    assert!(matches!(
        events.last().map(|event| &event.payload),
        Some(AgentEventPayload::TurnError(error))
            if error.error.code == talos_error::ErrorCode::InternalError
    ));
}

#[tokio::test]
async fn project_delete_is_blocked_until_owned_files_are_removed() {
    let (service, _store) = test_application(
        Arc::new(TestTurnExecutor {
            mode: TestTurnExecutorMode::Closed,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    let directory = tempfile::tempdir().unwrap();
    let project = service
        .create_project(
            &actor,
            "Project with file".to_string(),
            directory.path().to_path_buf(),
        )
        .await
        .unwrap();
    let uploaded = service
        .files
        .upload(
            &actor,
            talos_types::FileUploadRequest {
                filename: "image.png".to_string(),
                scope: talos_types::FileScope::Project {
                    project_id: project.project_id.clone(),
                },
                tags: Vec::new(),
                metadata: BTreeMap::new(),
            },
            Box::pin(stream::iter([Ok(Bytes::from_static(
                b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR",
            ))])),
        )
        .await
        .unwrap();

    let conflict = service
        .delete_project(&actor, &project.project_id)
        .await
        .unwrap_err();
    assert!(conflict.is_code(talos_error::ErrorCode::Conflict));

    service
        .files
        .delete(
            &actor,
            talos_types::FileDeleteRequest {
                file_id: uploaded.file.file_id,
            },
        )
        .await
        .unwrap();
    service
        .delete_project(&actor, &project.project_id)
        .await
        .unwrap();
}
