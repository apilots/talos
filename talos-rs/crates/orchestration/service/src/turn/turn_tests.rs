use super::*;
use crate::session::SessionRecordService;
use crate::TimelineEventRetentionConfig;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use talos_storage::SqliteStorage;
use talos_store::session::{
    ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, DatabaseSessionStore,
    MessageIdempotencyStore, PendingDynamicToolResult, PendingUserAction, SessionStore,
    TimelineEventId, TimelineEventStore,
};
use talos_types::talos::{
    StopReason, TalosContext, TalosProvider, TalosSourceType, TalosTurnCompletedEvent, TalosUsage,
};
use talos_types::{
    AgentDispatchRequest, SessionConf, SessionContext, SessionState, StartTurnRequest, TalosConfig,
    TalosSession,
};

#[derive(Default)]
struct RecordingTurnExecutor {
    stream_request: Mutex<Option<AgentDispatchRequest>>,
    title_update: Mutex<Option<String>>,
}

struct NoopSessionRuntime;

#[async_trait]
impl SessionRuntime for NoopSessionRuntime {
    async fn compact_session(
        &self,
        _session: talos_types::TalosSession,
    ) -> TalosResult<talos_types::TalosUsage> {
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

    async fn cancel_active_turn(&self, _session: talos_types::TalosSession) -> TalosResult<()> {
        unreachable!("not used in turn service tests")
    }

    async fn submit_dynamic_tool_result(
        &self,
        _session: talos_types::TalosSession,
        _call_id: &str,
        _tool_name: &str,
        _output: String,
        _success: bool,
    ) -> TalosResult<()> {
        unreachable!("not used in turn service tests")
    }
}

#[async_trait]
impl TurnExecutor for RecordingTurnExecutor {
    async fn start_turn_stream(
        &self,
        request: AgentDispatchRequest,
    ) -> TalosResult<mpsc::Receiver<TalosTimelineEvent>> {
        self.stream_request.lock().unwrap().replace(request.clone());
        let (tx, rx) = mpsc::channel(4);
        let event = TalosTimelineEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: None,
            event_type: talos_types::talos::TalosTimelineEventType::TurnCompleted,
            turn_id: request.turn_id.clone().unwrap_or_default(),
            message_id: request.context.message_id.clone(),
            session_id: request.session_id.clone(),
            sequence: 0,
            payload: TalosTimelineEventPayload::TurnCompleted(TalosTurnCompletedEvent {
                request_id: "req-test".to_string(),
                turn_id: request.turn_id.unwrap_or_default(),
                completed_at_ms: 1,
                stop_reason: StopReason::EndTurn,
                usage: TalosUsage::default(),
                metrics: None,
            }),
        };
        tx.send(event).await.unwrap();
        Ok(rx)
    }

    async fn cancel_active_turn(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_dynamic_tool_result(
        &self,
        _session: TalosSession,
        _call_id: &str,
        _tool_name: &str,
        _output: String,
        _success: bool,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn take_session_title_update(&self, _session_id: &str, _turn_id: &str) -> Option<String> {
        self.title_update.lock().unwrap().take()
    }
}

async fn test_service(
    turn_executor: Arc<RecordingTurnExecutor>,
) -> (
    TurnService,
    Arc<TimelineEventService>,
    Arc<DatabaseSessionStore>,
) {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    let store = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
    let event_store: Arc<dyn TimelineEventStore> = store.clone();
    let active_turn_store: Arc<dyn ActiveTurnStore> = store.clone();
    let event_service = Arc::new(TimelineEventService::new(
        event_store,
        active_turn_store,
        TimelineEventRetentionConfig::default(),
    ));
    let idempotency_store: Arc<dyn MessageIdempotencyStore> = store.clone();
    let session_store: Arc<dyn SessionStore> = store.clone();
    let session_runtime: Arc<dyn SessionRuntime> = Arc::new(NoopSessionRuntime);
    (
        TurnService::new(
            turn_executor,
            event_service.clone(),
            idempotency_store,
            session_store,
            session_runtime,
            TalosConfig::default(),
        ),
        event_service,
        store,
    )
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
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
        config: TalosConfig::default(),
        state: SessionState::default(),
    }
}

async fn create_test_session_record(store: Arc<DatabaseSessionStore>) {
    let session = test_session();
    let context = TalosContext {
        source_type: session.context.source_type,
        provider: session.context.provider,
        user_id: session.context.user_id,
        message_id: session.context.message_id,
        trace_id: session.context.trace_id,
        channel_id: session.context.channel_id,
        thread_id: session.context.thread_id,
        project_root: session.context.project_root,
        environment: session.context.environment,
    };
    SessionRecordService::new(store)
        .create_session_record(
            Some("s1"),
            &context,
            "default",
            "default-model",
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("session record should be created");
}

#[tokio::test]
async fn submit_start_turn_records_completed_turn_progress() {
    let turn_executor = Arc::new(RecordingTurnExecutor::default());
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
                model: None,
                project_id: None,
                current_directory: None,
                client_request_id: None,
            },
            None,
        )
        .await
        .expect("message should be submitted");

    let record = store
        .get_session_record("s1")
        .await
        .expect("session record should be readable")
        .expect("session record should exist");
    assert_eq!(record.message_count, 2);
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
        model: None,
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

    assert!(matches!(error, TalosError::IdempotencyError(_)));
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
                model: None,
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
    assert_eq!(request.turn_id.as_deref(), Some("turn-original"));
    assert_eq!(request.context.message_id, "msg-original");
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
