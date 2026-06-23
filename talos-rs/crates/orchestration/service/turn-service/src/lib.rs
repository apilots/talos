use std::sync::Arc;

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use talos_coordinator::SessionRuntime;
use talos_error::{TalosError, TalosResult};
use talos_session_service::{SessionRecordService, TimelineEventService};
use talos_store::session::{IdempotencyRecord, MessageCommandResult};
use talos_types::agent_dispatch::AgentDispatchRequest;
use talos_types::talos::{TalosInputItem, TalosTimelineEvent, TalosTimelineEventPayload};
use talos_types::{AgentDispatchRuntime, TalosSession};
use tokio::sync::mpsc;
use tracing::error;

#[async_trait]
pub trait TurnExecutor: Send + Sync {
    async fn start_turn_stream(
        &self,
        request: AgentDispatchRequest,
    ) -> TalosResult<mpsc::Receiver<TalosTimelineEvent>>;

    async fn cancel_active_turn(&self, session: TalosSession) -> TalosResult<()>;

    async fn submit_dynamic_tool_result(
        &self,
        session: TalosSession,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()>;

    async fn take_session_title_update(&self, _session_id: &str, _turn_id: &str) -> Option<String> {
        None
    }
}

#[derive(Clone)]
pub struct AgentDispatchTurnExecutor {
    handler: Arc<dyn AgentDispatchRuntime>,
}

impl AgentDispatchTurnExecutor {
    pub fn new(handler: Arc<dyn AgentDispatchRuntime>) -> Self {
        Self { handler }
    }
}

#[async_trait]
impl TurnExecutor for AgentDispatchTurnExecutor {
    async fn start_turn_stream(
        &self,
        request: AgentDispatchRequest,
    ) -> TalosResult<mpsc::Receiver<TalosTimelineEvent>> {
        self.handler.handle_stream(request).await
    }

    async fn cancel_active_turn(&self, session: TalosSession) -> TalosResult<()> {
        self.handler.cancel_active_turn(session).await
    }

    async fn submit_dynamic_tool_result(
        &self,
        session: TalosSession,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        self.handler
            .submit_dynamic_tool_result(session, call_id, tool_name, output, success)
            .await
    }

    async fn take_session_title_update(&self, session_id: &str, turn_id: &str) -> Option<String> {
        self.handler
            .take_session_title_update(session_id, turn_id)
            .await
    }
}

mod service;
pub use service::TurnService;

#[derive(Debug, Clone)]
pub struct TurnRetryRequest {
    pub input: Vec<TalosInputItem>,
    pub workspace_id: Option<String>,
    pub client_message_id: String,
    pub retry_of_message_id: String,
}

fn validate_text_input(input: &[TalosInputItem]) -> TalosResult<()> {
    let has_text = input.iter().any(|item| match item {
        TalosInputItem::Text { text } => !text.trim().is_empty(),
        _ => false,
    });
    if has_text {
        return Ok(());
    }
    Err(TalosError::ValidationError(
        "input must include at least one non-empty text item".to_string(),
    ))
}

fn validate_retry_request(request: &TurnRetryRequest) -> TalosResult<()> {
    if request.client_message_id.trim().is_empty() {
        return Err(TalosError::ValidationError(
            "client_message_id is required".to_string(),
        ));
    }
    if request.retry_of_message_id.trim().is_empty() {
        return Err(TalosError::ValidationError(
            "retry_of_message_id is required".to_string(),
        ));
    }
    Ok(())
}

fn retry_idempotency_key(request: &TurnRetryRequest) -> String {
    format!(
        "retry_{}_{}",
        request.client_message_id, request.retry_of_message_id
    )
}

fn result_from_idempotency_record(record: IdempotencyRecord) -> TalosResult<MessageCommandResult> {
    record.result.ok_or_else(|| {
        TalosError::IdempotencyError(format!(
            "idempotency record is missing result: session_id={}, key={}, status={}",
            record.session_id,
            record.idempotency_key,
            record.status.as_str()
        ))
    })
}

fn spawn_event_forwarder(
    event_service: Arc<TimelineEventService>,
    turn_executor: Arc<dyn TurnExecutor>,
    session_records: Arc<SessionRecordService>,
    session_runtime: Arc<dyn SessionRuntime>,
    assistant_model: String,
    initial_completed_turn: Option<(String, String)>,
    mut stream_rx: mpsc::Receiver<TalosTimelineEvent>,
) {
    tokio::spawn(async move {
        let mut completed_turn = initial_completed_turn;
        while let Some(event) = stream_rx.recv().await {
            let progress_event = event.clone();
            if let Err(err) = event_service.record_talos_event(event).await {
                error!(error = %err, "failed to persist timeline stream event");
                continue;
            }
            record_completed_turn_progress(
                session_records.as_ref(),
                session_runtime.as_ref(),
                &progress_event,
                &assistant_model,
            )
            .await;
            if let Some(turn) = completed_turn_from_event(&progress_event) {
                completed_turn = Some(turn);
            }
        }

        if let Some((session_id, turn_id)) = completed_turn {
            if let Some(title) = turn_executor
                .take_session_title_update(&session_id, &turn_id)
                .await
            {
                record_session_title(
                    session_records.as_ref(),
                    session_runtime.as_ref(),
                    &session_id,
                    &title,
                )
                .await;
            }
        }
    });
}

fn completed_turn_from_event(event: &TalosTimelineEvent) -> Option<(String, String)> {
    matches!(event.payload, TalosTimelineEventPayload::TurnCompleted(_))
        .then(|| (event.session_id.clone(), event.turn_id.clone()))
}

async fn record_session_title(
    session_records: &SessionRecordService,
    session_runtime: &dyn SessionRuntime,
    session_id: &str,
    title: &str,
) {
    if let Err(err) = session_records
        .update_session_title(session_id, title)
        .await
    {
        error!(
            session_id = %session_id,
            error = %err,
            "failed to persist session title"
        );
        return;
    }
    if let Err(err) = session_runtime.invalidate_cached_session(session_id).await {
        error!(
            session_id = %session_id,
            error = %err,
            "failed to invalidate session cache after title update"
        );
    }
}

async fn record_completed_turn_progress(
    session_records: &SessionRecordService,
    session_runtime: &dyn SessionRuntime,
    event: &TalosTimelineEvent,
    assistant_model: &str,
) {
    let TalosTimelineEventPayload::TurnCompleted(completed) = &event.payload else {
        return;
    };
    if let Err(err) = session_records
        .record_session_progress(
            &event.session_id,
            2,
            &completed.usage,
            Some(assistant_model),
        )
        .await
    {
        error!(
            session_id = %event.session_id,
            turn_id = %event.turn_id,
            error = %err,
            "failed to persist completed turn session progress"
        );
        return;
    }
    if let Err(err) = session_runtime
        .invalidate_cached_session(&event.session_id)
        .await
    {
        error!(
            session_id = %event.session_id,
            turn_id = %event.turn_id,
            error = %err,
            "failed to invalidate session cache after turn progress"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use talos_session_service::{SessionRecordService, TimelineEventRetentionConfig};
    use talos_storage::SqliteStorage;
    use talos_store::session::{
        ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, DatabaseSessionStore,
        MessageIdempotencyStore, PendingDynamicToolResult, PendingUserAction, SessionStore,
        TimelineEventId, TimelineEventStore,
    };
    use talos_types::talos::{
        StopReason, TalosContext, TalosProvider, TalosSourceType, TalosTurnCompletedEvent,
        TalosUsage,
    };
    use talos_types::{SessionConf, SessionContext, SessionState, StartTurnRequest, TalosConfig};

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

        async fn prepare_delete_session(
            &self,
            _session: talos_types::TalosSession,
        ) -> TalosResult<()> {
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

        async fn take_session_title_update(
            &self,
            _session_id: &str,
            _turn_id: &str,
        ) -> Option<String> {
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
        let request_hash =
            turn_submission_hash("s1", &request.input, &request.workspace_id).unwrap();
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
}

pub fn turn_submission_hash(
    session_id: &str,
    input: &[TalosInputItem],
    workspace_id: &Option<String>,
) -> TalosResult<String> {
    let canonical = serde_json::json!({
        "session_id": session_id,
        "workspace_id": workspace_id,
        "input": input,
    });
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|err| TalosError::SerializationError(err.to_string()))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}
