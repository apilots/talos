use super::*;
use crate::session::SessionService;
use crate::test_support::run_test_migrations;
use crate::{TimelineEventRetentionConfig, TimelineEventService};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use talos_storage::SqliteStorage;
use talos_store::profile::{DatabaseProfileStore, ProfileStore};
use talos_store::project::MockProjectStore;
use talos_store::session::{
    ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, DatabaseSessionStore,
    MessageIdempotencyStore, PendingDynamicToolResult, PendingUserAction, SessionStore,
    TimelineEventId, TimelineEventStore,
};
use talos_types::talos::{
    StopReason, TalosProvider, TalosSourceType, TalosTurnCompletedEvent, TokenUsage,
};
use talos_types::{
    AgentDispatchInput, AgentDispatchInputParams, AgentDispatchParams, ModelRef, ModelSelection,
    SessionConf, SessionContext, SessionState, StartTurnRequest, TalosConfig, TalosSession,
};
use tokio::sync::mpsc;

#[test]
fn turn_input_accepts_files_without_text() {
    assert!(validate_user_input(&[TalosInputItem::Files {
        file_ids: vec!["file-1".to_string()],
    }])
    .is_ok());
}

#[test]
fn turn_input_rejects_ambiguous_or_duplicate_file_references() {
    assert!(validate_user_input(&[
        TalosInputItem::Files {
            file_ids: vec!["file-1".to_string()],
        },
        TalosInputItem::Files {
            file_ids: vec!["file-2".to_string()],
        },
    ])
    .is_err());
    assert!(validate_user_input(&[TalosInputItem::Files {
        file_ids: vec!["file-1".to_string(), "file-1".to_string()],
    }])
    .is_err());
}

#[derive(Default)]
struct RecordingTurnExecutor {
    stream_request: Mutex<Option<AgentDispatchParams>>,
    stream_error: Mutex<Option<TalosError>>,
    dynamic_turn_id: Mutex<Option<String>>,
    title_update: Mutex<Option<String>>,
    completed_stop_reason: Mutex<Option<StopReason>>,
}

struct NoopSessionRuntime;

#[async_trait]
impl SessionRuntime for NoopSessionRuntime {
    async fn compact_session(
        &self,
        _params: talos_coordinator::AgentSessionCompactionParams,
    ) -> TalosResult<talos_types::TokenUsage> {
        unreachable!("not used in turn service tests")
    }

    async fn clear_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        unreachable!("not used in turn service tests")
    }

    async fn close_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        unreachable!("not used in turn service tests")
    }

    async fn prepare_delete_session(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        unreachable!("not used in turn service tests")
    }

    async fn cancel_active_turn(
        &self,
        _params: talos_types::CancelActiveTurnParams,
    ) -> TalosResult<()> {
        unreachable!("not used in turn service tests")
    }
}

#[async_trait]
impl TurnExecutor for RecordingTurnExecutor {
    async fn validate_agent_id(&self, _agent_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn start_turn_stream(
        &self,
        request: AgentDispatchParams,
    ) -> TalosResult<mpsc::Receiver<AgentEvent>> {
        self.stream_request.lock().unwrap().replace(request.clone());
        if let Some(error) = self.stream_error.lock().unwrap().take() {
            return Err(error);
        }
        let (tx, rx) = mpsc::channel(4);
        let event = AgentEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: None,
            event_type: talos_types::talos::AgentEventType::TurnCompleted,
            turn_id: request.turn_id.clone(),
            message_id: request.session.context.message_id.clone(),
            session_id: request.session.context.session_id.clone(),
            sequence: 0,
            payload: AgentEventPayload::TurnCompleted(TalosTurnCompletedEvent {
                request_id: "req-test".to_string(),
                turn_id: request.turn_id,
                completed_at_ms: 1,
                stop_reason: self
                    .completed_stop_reason
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap_or(StopReason::EndTurn),
                usage: TokenUsage::default(),
                metrics: None,
                artifact_seq: Some(1),
            }),
        };
        tx.send(event).await.unwrap();
        Ok(rx)
    }

    async fn submit_input(&self, params: AgentDispatchInputParams) -> TalosResult<()> {
        assert!(matches!(
            params.binding,
            talos_types::DispatchBinding::Exact { ref agent_id } if agent_id == "default"
        ));
        assert!(matches!(
            params.input,
            AgentDispatchInput::DynamicToolResult { .. }
        ));
        self.dynamic_turn_id.lock().unwrap().replace(params.turn_id);
        Ok(())
    }

    async fn take_generated_session_title(
        &self,
        _session_id: &str,
        _turn_id: &str,
    ) -> Option<String> {
        self.title_update.lock().unwrap().take()
    }
}

#[tokio::test]
async fn submit_start_turn_preserves_executor_error_semantics() {
    let turn_executor = Arc::new(RecordingTurnExecutor {
        stream_error: Mutex::new(Some(TalosError::public(
            talos_error::ErrorCode::Conflict,
            "agent busy".to_string(),
        ))),
        ..Default::default()
    });
    let (service, _event_service, store) = test_service(turn_executor).await;
    create_test_session_record(store.clone()).await;

    let error = service
        .submit_start_turn(
            test_session(),
            StartTurnRequest {
                session_id: "s1".to_string(),
                input: vec![TalosInputItem::Text {
                    text: "hello".to_string(),
                }],
                workspace_id: None,
                context: None,
                model_selection: None,
                project_id: None,
                current_directory: None,
                client_request_id: None,
            },
            None,
        )
        .await
        .expect_err("executor conflict should propagate");

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
}

async fn test_service(
    turn_executor: Arc<RecordingTurnExecutor>,
) -> (
    TurnService,
    Arc<TimelineEventService>,
    Arc<DatabaseSessionStore>,
) {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    run_test_migrations(storage.pool()).await;
    let profile_store: Arc<dyn ProfileStore> =
        Arc::new(DatabaseProfileStore::new(storage.pool().clone()));
    let store = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
    let event_store: Arc<dyn TimelineEventStore> = store.clone();
    let active_turn_store: Arc<dyn ActiveTurnStore> = store.clone();
    let event_service = Arc::new(TimelineEventService::new(
        event_store,
        active_turn_store,
        TimelineEventRetentionConfig::default(),
    ));
    let idempotency_store: Arc<dyn MessageIdempotencyStore> = store.clone();
    let config = test_config();
    let sessions = Arc::new(SessionService::new(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config.clone(),
    ));
    let session_runtime: Arc<dyn SessionRuntime> = Arc::new(NoopSessionRuntime);
    (
        TurnService::new(
            turn_executor,
            event_service.clone(),
            idempotency_store,
            profile_store,
            sessions,
            session_runtime,
            config,
        ),
        event_service,
        store,
    )
}

fn test_model_selection() -> ModelSelection {
    ModelSelection::without_reasoning(ModelRef {
        provider: "anthropic".to_string(),
        id: "test-model".to_string(),
    })
}

fn test_config() -> TalosConfig {
    let mut config = TalosConfig::default();
    config.llm.model = "test-model".to_string();
    config.llm.anthropic.models = vec![talos_config::ProviderModelConfig {
        id: "test-model".to_string(),
        name: "Test model".to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }];
    config
}

fn test_session() -> TalosSession {
    TalosSession {
        title: "Untitled Session".to_string(),
        user_id: "user-1".to_string(),
        context: SessionContext {
            session_id: "s1".to_string(),
            workspace_id: None,
            source_type: TalosSourceType::Http,
            provider: TalosProvider::Http,
            user_id: "user-1".to_string(),
            message_id: "stale-message".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: None,
            thread_id: None,
            project_id: None,
            current_directory: None,
            project_root: None,
            environment: None,
        },
        conf: SessionConf {
            model_selection: Some(test_model_selection()),
        },
        project_id: None,
        current_directory: None,
        config: test_config(),
        state: SessionState::default(),
    }
}

async fn create_test_session_record(store: Arc<DatabaseSessionStore>) {
    let session = test_session();
    let record = talos_store::session::SessionRecord::from_session(&session, "default");
    store
        .insert_session_record(&record, "api")
        .await
        .expect("session record should be created");
}

#[tokio::test]
async fn submit_start_turn_records_completed_turn_progress() {
    let turn_executor = Arc::new(RecordingTurnExecutor::default());
    let (service, _event_service, store) = test_service(turn_executor.clone()).await;
    create_test_session_record(store.clone()).await;

    service
        .submit_start_turn(
            test_session(),
            StartTurnRequest {
                session_id: "s1".to_string(),
                input: vec![TalosInputItem::Text {
                    text: "hello".to_string(),
                }],
                workspace_id: None,
                context: None,
                model_selection: None,
                project_id: None,
                current_directory: None,
                client_request_id: None,
            },
            None,
        )
        .await
        .expect("message should be submitted");

    assert!(matches!(
        turn_executor
            .stream_request
            .lock()
            .unwrap()
            .as_ref()
            .map(|request| &request.binding),
        Some(talos_types::DispatchBinding::Exact { agent_id }) if agent_id == "default"
    ));
    let record = store
        .get_session_record("s1")
        .await
        .expect("session record should be readable")
        .expect("session record should exist");
    assert_eq!((record.message_count, record.latest_turn_seq), (2, 1));
}

#[tokio::test]
async fn cancelled_turn_does_not_advance_session_progress() {
    let turn_executor = Arc::new(RecordingTurnExecutor::default());
    turn_executor
        .completed_stop_reason
        .lock()
        .unwrap()
        .replace(StopReason::Cancelled);
    let (service, _event_service, store) = test_service(turn_executor).await;
    create_test_session_record(store.clone()).await;

    service
        .submit_start_turn(
            test_session(),
            StartTurnRequest {
                session_id: "s1".to_string(),
                input: vec![TalosInputItem::Text {
                    text: "hello".to_string(),
                }],
                workspace_id: None,
                context: None,
                model_selection: None,
                project_id: None,
                current_directory: None,
                client_request_id: None,
            },
            None,
        )
        .await
        .expect("cancelled turn should complete its command lifecycle");

    let record = store
        .get_session_record("s1")
        .await
        .expect("session record should be readable")
        .expect("session record should exist");
    assert_eq!((record.message_count, record.latest_turn_seq), (0, 0));
}

#[tokio::test]
async fn submit_start_turn_fails_when_existing_idempotency_record_has_no_result() {
    let turn_executor = Arc::new(RecordingTurnExecutor::default());
    let (service, _event_service, store) = test_service(turn_executor.clone()).await;
    create_test_session_record(store.clone()).await;
    let request = StartTurnRequest {
        session_id: "s1".to_string(),
        input: vec![TalosInputItem::Text {
            text: "hello".to_string(),
        }],
        workspace_id: None,
        context: None,
        model_selection: None,
        project_id: None,
        current_directory: None,
        client_request_id: None,
    };
    let request_hash = turn_submission_hash("s1", &request.input, &request.workspace_id).unwrap();
    store
        .reserve(
            "s1",
            "missing-result",
            &request_hash,
            "turn-existing",
            1,
            None,
        )
        .await
        .expect("idempotency reservation should be inserted");

    let error = service
        .submit_start_turn(test_session(), request, Some("missing-result".to_string()))
        .await
        .expect_err("missing idempotency result should fail fast");

    assert!(error.is_internal());
    assert!(turn_executor.stream_request.lock().unwrap().is_none());
}

#[tokio::test]
async fn submit_start_turn_persists_agent_title_update() {
    let turn_executor = Arc::new(RecordingTurnExecutor::default());
    turn_executor
        .title_update
        .lock()
        .unwrap()
        .replace("Meaningful Topic".to_string());
    let (service, _event_service, store) = test_service(turn_executor).await;
    create_test_session_record(store.clone()).await;

    service
        .submit_start_turn(
            test_session(),
            StartTurnRequest {
                session_id: "s1".to_string(),
                input: vec![TalosInputItem::Text {
                    text: "hello".to_string(),
                }],
                workspace_id: None,
                context: None,
                model_selection: None,
                project_id: None,
                current_directory: None,
                client_request_id: None,
            },
            None,
        )
        .await
        .expect("message should be submitted");

    for _ in 0..20 {
        let record = store
            .get_session_record("s1")
            .await
            .expect("session record should be readable")
            .expect("session record should exist");
        if record.title == "Meaningful Topic" {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let record = store
        .get_session_record("s1")
        .await
        .expect("session record should be readable")
        .expect("session record should exist");
    assert_eq!(record.title, "Meaningful Topic");
}

#[tokio::test]
async fn submit_dynamic_tool_result_resumes_original_turn_and_message() {
    let turn_executor = Arc::new(RecordingTurnExecutor::default());
    let (service, _event_service, store) = test_service(turn_executor.clone()).await;
    create_test_session_record(store.clone()).await;
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "s1".to_string(),
            turn_id: "turn-original".to_string(),
            message_id: Some("msg-original".to_string()),
            status: ActiveTurnStatus::WaitingUserAction,
            last_event_id: Some(TimelineEventId::from_seq(9)),
            pending_user_action: Some(PendingUserAction::DynamicToolResult(
                PendingDynamicToolResult {
                    item_id: Some("tool-original".to_string()),
                    call_id: "call-1".to_string(),
                    tool_name: "AskUser".to_string(),
                },
            )),
            updated_at: 1,
        })
        .await
        .unwrap();

    service
        .submit_dynamic_tool_result(test_session(), "call-1", "AskUser", "ok".to_string(), true)
        .await
        .unwrap();

    let request = turn_executor
        .stream_request
        .lock()
        .unwrap()
        .clone()
        .expect("resume stream request should be captured");
    assert_eq!(request.turn_id, "turn-original");
    assert_eq!(
        turn_executor.dynamic_turn_id.lock().unwrap().as_deref(),
        Some("turn-original")
    );
    assert_eq!(request.session.context.message_id, "msg-original");
    assert!(request.input.is_empty());
    let tool_call = request
        .resume_context
        .and_then(|context| context.tool_call)
        .expect("resume tool call should be present");
    assert_eq!(tool_call.item_id, "tool-original");
    assert_eq!(tool_call.call_id, "call-1");
    assert_eq!(tool_call.tool_name, "AskUser");

    let active = store.get_active_turn("s1").await.unwrap().unwrap();
    assert_eq!(active.status, ActiveTurnStatus::Completed);
    assert_eq!(active.pending_user_action, None);
}

#[tokio::test]
async fn submit_dynamic_tool_result_requires_original_tool_item_id() {
    let turn_executor = Arc::new(RecordingTurnExecutor::default());
    let (service, _event_service, store) = test_service(turn_executor.clone()).await;
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "s1".to_string(),
            turn_id: "turn-original".to_string(),
            message_id: Some("msg-original".to_string()),
            status: ActiveTurnStatus::WaitingUserAction,
            last_event_id: Some(TimelineEventId::from_seq(9)),
            pending_user_action: Some(PendingUserAction::DynamicToolResult(
                PendingDynamicToolResult {
                    item_id: None,
                    call_id: "call-1".to_string(),
                    tool_name: "AskUser".to_string(),
                },
            )),
            updated_at: 1,
        })
        .await
        .unwrap();

    let error = service
        .submit_dynamic_tool_result(test_session(), "call-1", "AskUser", "ok".to_string(), true)
        .await
        .expect_err("missing item id should fail before runtime dispatch");

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
    assert!(turn_executor.stream_request.lock().unwrap().is_none());
    assert!(turn_executor.dynamic_turn_id.lock().unwrap().is_none());
    let active = store.get_active_turn("s1").await.unwrap().unwrap();
    assert_eq!(active.status, ActiveTurnStatus::WaitingUserAction);
    assert!(active.pending_user_action.is_some());
}
