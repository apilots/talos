use super::*;
use crate::session::SessionService;
use crate::test_support::run_test_migrations;
use crate::{SessionMessageQueueService, TimelineEventRetentionConfig, TimelineEventService};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use talos_error::ApiError;
use talos_storage::SqliteStorage;
use talos_store::profile::{DatabaseProfileStore, ProfileStore};
use talos_store::project::MockProjectStore;
use talos_store::session::{
    ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, DatabaseSessionStore,
    MessageIdempotencyStore, SessionStore, TimelineEventId, TimelineEventStore, UserActionRecord,
    UserActionStore,
};
use talos_types::agent_dispatch::AgentDispatchResumeToolCall;
use talos_types::talos::{
    StopReason, TalosItemDeltaEvent, TalosItemStartedEvent, TalosProvider, TalosSourceType,
    TalosTimelineContent, TalosTimelineDelta, TalosTimelineItem, TalosTimelineItemKind,
    TalosTimelineItemStatus, TalosTimelinePhase, TalosTurnCompletedEvent, TokenUsage,
    TurnExecutionMetrics,
};
use talos_types::{
    AgentDispatchInput, AgentDispatchInputParams, AgentDispatchParams, DynamicToolResult,
    DynamicToolResultParams, ModelRef, ModelSelection, PendingUserAction, SandboxApprovalDecision,
    SandboxApprovalParams, SandboxApprovalPresentation, SandboxApprovalReason, SessionConf,
    SessionContext, SessionState, StartTurnRequest, TalosConfig, TalosSession,
    TimelineUserActionEvent, UserActionParams, UserActionResponse, UserActionState,
};
use tokio::sync::mpsc;

#[path = "queue_start_tests.rs"]
mod queue_start_tests;

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

#[test]
fn resume_context_keeps_all_nonterminal_tool_item_identities_in_event_order() {
    let mut timeline =
        talos_agent_kernel::TimelineBuilder::new("request-1", "turn-1", "message-1", "session-1");
    let first = timeline.start_tool_call("call-1", "Bash", "{}");
    let second = timeline.start_tool_call("call-2", "Read", "{}");
    let third = timeline.start_tool_call("call-3", "Read", "{}");
    let mut events = vec![first, second, third];
    events.push(
        timeline
            .append_delta(
                "tool-1",
                TalosTimelineDelta::Status {
                    status: TalosTimelineItemStatus::WaitingUserAction,
                },
            )
            .expect("first tool should wait"),
    );
    events.push(
        timeline
            .append_delta(
                "tool-2",
                TalosTimelineDelta::Status {
                    status: TalosTimelineItemStatus::Queued,
                },
            )
            .expect("second tool should queue"),
    );
    events.push(
        timeline
            .complete_item("tool-3", TalosTimelineItemStatus::Completed, None)
            .expect("third tool should complete"),
    );

    assert_eq!(
        service::nonterminal_resume_tool_calls(&events),
        vec![
            AgentDispatchResumeToolCall {
                item_id: "tool-1".to_string(),
                call_id: "call-1".to_string(),
                tool_name: "Bash".to_string(),
            },
            AgentDispatchResumeToolCall {
                item_id: "tool-2".to_string(),
                call_id: "call-2".to_string(),
                tool_name: "Read".to_string(),
            },
        ]
    );
}

#[derive(Default)]
struct RecordingTurnExecutor {
    stream_request: Mutex<Option<AgentDispatchParams>>,
    stream_requests: Mutex<Vec<AgentDispatchParams>>,
    stream_error: Mutex<Option<TalosError>>,
    input_error: Mutex<Option<TalosError>>,
    submitted_inputs: Mutex<Vec<AgentDispatchInputParams>>,
    turn_errors_remaining: Mutex<usize>,
    dynamic_turn_id: Mutex<Option<String>>,
    title_update: Mutex<Option<String>>,
    completed_stop_reason: Mutex<Option<StopReason>>,
}

#[derive(Default)]
struct NoopSessionRuntime {
    cancelled_turns: Mutex<Vec<String>>,
}

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
        params: talos_types::CancelActiveTurnParams,
    ) -> TalosResult<talos_types::CancelActiveTurnOutcome> {
        self.cancelled_turns.lock().unwrap().push(params.turn_id);
        Ok(talos_types::CancelActiveTurnOutcome::Cancelled)
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
        self.stream_requests.lock().unwrap().push(request.clone());
        if let Some(error) = self.stream_error.lock().unwrap().take() {
            return Err(error);
        }
        let (tx, rx) = mpsc::channel(4);
        if let Some(item_id) = request
            .resume_context
            .as_ref()
            .and_then(|context| {
                context
                    .tool_calls
                    .iter()
                    .find(|tool_call| tool_call.call_id == context.pending_call_id)
            })
            .map(|tool_call| tool_call.item_id.clone())
        {
            tx.send(AgentEvent {
                event_seq: None,
                event_id: None,
                created_at_ms: None,
                event_type: talos_types::talos::AgentEventType::ItemDelta,
                turn_id: request.turn_id.clone(),
                message_id: request.session.context.message_id.clone(),
                session_id: request.session.context.session_id.clone(),
                sequence: 0,
                payload: AgentEventPayload::ItemDelta(talos_types::talos::TalosItemDeltaEvent {
                    request_id: "req-test".to_string(),
                    turn_id: request.turn_id.clone(),
                    item_id,
                    delta: talos_types::talos::TalosTimelineDelta::ToolResult {
                        content: vec![talos_types::talos::TalosContentItem::Text {
                            text: "resumed".to_string(),
                        }],
                        is_error: false,
                    },
                }),
            })
            .await
            .unwrap();
        }
        let should_error = {
            let mut turn_errors_remaining = self.turn_errors_remaining.lock().unwrap();
            let should_error = *turn_errors_remaining > 0;
            *turn_errors_remaining = turn_errors_remaining.saturating_sub(1);
            should_error
        };
        let (event_type, payload) = if should_error {
            (
                talos_types::talos::AgentEventType::TurnError,
                AgentEventPayload::TurnError(talos_types::talos::TalosTurnErrorEvent {
                    request_id: "req-test".to_string(),
                    turn_id: request.turn_id.clone(),
                    error: ApiError::new(talos_error::ErrorCode::InternalError),
                    failed_at_ms: 1,
                }),
            )
        } else {
            (
                talos_types::talos::AgentEventType::TurnCompleted,
                AgentEventPayload::TurnCompleted(TalosTurnCompletedEvent {
                    request_id: "req-test".to_string(),
                    turn_id: request.turn_id.clone(),
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
            )
        };
        let event = AgentEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: None,
            event_type,
            turn_id: request.turn_id.clone(),
            message_id: request.session.context.message_id.clone(),
            session_id: request.session.context.session_id.clone(),
            sequence: 1,
            payload,
        };
        tx.send(event).await.unwrap();
        Ok(rx)
    }

    async fn submit_input(&self, params: AgentDispatchInputParams) -> TalosResult<()> {
        if let Some(error) = self.input_error.lock().unwrap().take() {
            return Err(error);
        }
        assert!(matches!(
            params.binding,
            talos_types::DispatchBinding::Exact { ref agent_id } if agent_id == "default"
        ));
        if matches!(params.input, AgentDispatchInput::UserActionResponse { .. }) {
            self.dynamic_turn_id
                .lock()
                .unwrap()
                .replace(params.turn_id.clone());
        }
        self.submitted_inputs.lock().unwrap().push(params);
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
    let (service, _event_service, store, _message_queue) = test_service(turn_executor).await;
    create_test_session_record(store.clone()).await;

    let error = service
        .submit_start_turn(
            test_session(),
            StartTurnRequest {
                session_id: "s1".to_string(),
                input: vec![TalosInputItem::Text {
                    text: "hello".to_string(),
                }],
                context: None,
                model_selection: None,
                user_permission_mode: None,
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
    SessionMessageQueueService,
) {
    test_service_with_runtime(turn_executor, Arc::new(NoopSessionRuntime::default())).await
}

async fn test_service_with_runtime(
    turn_executor: Arc<RecordingTurnExecutor>,
    session_runtime: Arc<dyn SessionRuntime>,
) -> (
    TurnService,
    Arc<TimelineEventService>,
    Arc<DatabaseSessionStore>,
    SessionMessageQueueService,
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
        store.clone() as Arc<dyn talos_store::session::UserActionStore>,
        TimelineEventRetentionConfig::default(),
    ));
    let idempotency_store: Arc<dyn MessageIdempotencyStore> = store.clone();
    let config = test_config();
    let sessions = Arc::new(SessionService::new(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config.clone(),
    ));
    let message_queue =
        SessionMessageQueueService::new(10).expect("queue service should initialize");
    (
        TurnService::new(
            turn_executor,
            event_service.clone(),
            idempotency_store,
            profile_store,
            sessions,
            session_runtime,
            message_queue.clone(),
            config,
        ),
        event_service,
        store,
        message_queue,
    )
}

async fn insert_pending_dynamic_action(store: &DatabaseSessionStore, item_id: Option<&str>) {
    store
        .create(UserActionRecord {
            session_id: "s1".to_string(),
            turn_id: "turn-original".to_string(),
            action: PendingUserAction {
                action_id: "action-1".to_string(),
                call_id: "call-1".to_string(),
                item_id: item_id.map(str::to_string),
                state: UserActionState::Pending,
                params: UserActionParams::DynamicToolResult(DynamicToolResultParams {
                    tool_name: "AskUser".to_string(),
                }),
            },
            response: None,
            version: 0,
            updated_at: 1,
        })
        .await
        .unwrap();
    if let Some(item_id) = item_id {
        insert_pending_tool_events(store, item_id, "call-1", "AskUser").await;
    }
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "s1".to_string(),
            turn_id: "turn-original".to_string(),
            message_id: Some("msg-original".to_string()),
            status: ActiveTurnStatus::WaitingUserAction,
            last_event_id: Some(TimelineEventId::from_seq(9)),
            current_action_id: Some("action-1".to_string()),
            updated_at: 1,
        })
        .await
        .unwrap();
}

async fn insert_pending_sandbox_approval(store: &DatabaseSessionStore) {
    store
        .create(UserActionRecord {
            session_id: "s1".to_string(),
            turn_id: "turn-original".to_string(),
            action: PendingUserAction {
                action_id: "action-1".to_string(),
                call_id: "call-1".to_string(),
                item_id: Some("tool-original".to_string()),
                state: UserActionState::Pending,
                params: UserActionParams::SandboxApproval(SandboxApprovalParams {
                    tool_name: "Bash".to_string(),
                    reason: SandboxApprovalReason::RiskyCommand,
                    presentation: SandboxApprovalPresentation::Command {
                        command: "rm -rf build".to_string(),
                        cwd: "/workspace".to_string(),
                        risk_summary: None,
                    },
                    available_decisions: vec![
                        SandboxApprovalDecision::AllowOnce,
                        SandboxApprovalDecision::Deny,
                        SandboxApprovalDecision::Cancel,
                    ],
                }),
            },
            response: None,
            version: 0,
            updated_at: 1,
        })
        .await
        .unwrap();
    insert_pending_tool_events(store, "tool-original", "call-1", "Bash").await;
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "s1".to_string(),
            turn_id: "turn-original".to_string(),
            message_id: Some("msg-original".to_string()),
            status: ActiveTurnStatus::WaitingUserAction,
            last_event_id: Some(TimelineEventId::from_seq(9)),
            current_action_id: Some("action-1".to_string()),
            updated_at: 1,
        })
        .await
        .unwrap();
}

async fn insert_pending_tool_events(
    store: &DatabaseSessionStore,
    item_id: &str,
    call_id: &str,
    tool_name: &str,
) {
    let base_event = |sequence, event_type, payload| AgentEvent {
        event_seq: None,
        event_id: None,
        created_at_ms: None,
        event_type,
        turn_id: "turn-original".to_string(),
        message_id: "msg-original".to_string(),
        session_id: "s1".to_string(),
        sequence,
        payload,
    };
    store
        .append_event(base_event(
            0,
            talos_types::talos::AgentEventType::ItemStarted,
            AgentEventPayload::ItemStarted(TalosItemStartedEvent {
                request_id: "request-original".to_string(),
                turn_id: "turn-original".to_string(),
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
            talos_types::talos::AgentEventType::ItemDelta,
            AgentEventPayload::ItemDelta(TalosItemDeltaEvent {
                request_id: "request-original".to_string(),
                turn_id: "turn-original".to_string(),
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
            talos_types::talos::AgentEventType::TurnCompleted,
            AgentEventPayload::TurnCompleted(TalosTurnCompletedEvent {
                request_id: "request-original".to_string(),
                turn_id: "turn-original".to_string(),
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
        agent_id: "default".to_string(),
        title: "Untitled Session".to_string(),
        user_id: "user-1".to_string(),
        context: SessionContext {
            session_id: "s1".to_string(),
            workspace_id: "default".to_string(),
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
            ..SessionConf::default()
        },
        project_id: None,
        current_directory: None,
        config: test_config(),
        effective_user_permission_mode: talos_types::UserPermissionMode::FullAccess,
        state: SessionState::default(),
    }
}

async fn create_test_session_record(store: Arc<DatabaseSessionStore>) {
    let session = test_session();
    let record = talos_store::session::SessionRecord::from_session(&session);
    store
        .insert_session_record(&record, "api")
        .await
        .expect("session record should be created");
}

async fn wait_for_stream_requests(executor: &RecordingTurnExecutor, expected: usize) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if executor.stream_requests.lock().unwrap().len() >= expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("queued turns should be dispatched");
}

#[tokio::test]
async fn queued_messages_auto_dispatch_in_fifo_order_after_fast_completion() {
    let turn_executor = Arc::new(RecordingTurnExecutor::default());
    let (service, _event_service, store, message_queue) = test_service(turn_executor.clone()).await;
    create_test_session_record(store).await;
    message_queue
        .add(
            "s1",
            vec![TalosInputItem::Text {
                text: "first".to_string(),
            }],
        )
        .unwrap();
    message_queue
        .add(
            "s1",
            vec![TalosInputItem::Text {
                text: "second".to_string(),
            }],
        )
        .unwrap();

    service
        .try_dispatch_next("s1")
        .await
        .expect("first queued message should start")
        .expect("queue should contain a message");
    wait_for_stream_requests(turn_executor.as_ref(), 2).await;

    let requests = turn_executor.stream_requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.input.clone())
            .collect::<Vec<_>>(),
        vec![
            vec![TalosInputItem::Text {
                text: "first".to_string(),
            }],
            vec![TalosInputItem::Text {
                text: "second".to_string(),
            }],
        ]
    );
    assert!(message_queue.list("s1").unwrap().messages.is_empty());
}

#[tokio::test]
async fn queued_messages_continue_after_a_failed_turn() {
    let turn_executor = Arc::new(RecordingTurnExecutor {
        turn_errors_remaining: Mutex::new(1),
        ..Default::default()
    });
    let (service, _event_service, store, message_queue) = test_service(turn_executor.clone()).await;
    create_test_session_record(store).await;
    message_queue
        .add(
            "s1",
            vec![TalosInputItem::Text {
                text: "fails".to_string(),
            }],
        )
        .unwrap();
    message_queue
        .add(
            "s1",
            vec![TalosInputItem::Text {
                text: "continues".to_string(),
            }],
        )
        .unwrap();

    service
        .try_dispatch_next("s1")
        .await
        .expect("failed queued turn should still be accepted");
    wait_for_stream_requests(turn_executor.as_ref(), 2).await;

    assert!(message_queue.list("s1").unwrap().messages.is_empty());
}

#[tokio::test]
async fn steer_accepts_text_and_files_but_rejects_skills_without_consuming_them() {
    let turn_executor = Arc::new(RecordingTurnExecutor::default());
    let (service, _event_service, store, message_queue) = test_service(turn_executor.clone()).await;
    create_test_session_record(store.clone()).await;
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "s1".to_string(),
            turn_id: "turn-active".to_string(),
            message_id: Some("msg-active".to_string()),
            status: ActiveTurnStatus::Running,
            last_event_id: None,
            current_action_id: None,
            updated_at: 1,
        })
        .await
        .unwrap();
    let accepted_input = vec![
        TalosInputItem::Text {
            text: "inspect this".to_string(),
        },
        TalosInputItem::Files {
            file_ids: vec!["file-1".to_string()],
        },
    ];
    let accepted = message_queue.add("s1", accepted_input.clone()).unwrap();

    let stale_error = service
        .steer_queued_message(test_session(), &accepted.messages[0].id, "turn-stale")
        .await
        .expect_err("stale turn id must not consume the queued message");
    assert!(stale_error.is_code(talos_error::ErrorCode::ResourceNotFound));
    assert_eq!(message_queue.list("s1").unwrap(), accepted);
    assert!(turn_executor.submitted_inputs.lock().unwrap().is_empty());

    service
        .steer_queued_message(test_session(), &accepted.messages[0].id, "turn-active")
        .await
        .expect("text and files should be accepted for steer");

    assert!(matches!(
        &turn_executor.submitted_inputs.lock().unwrap()[0].input,
        AgentDispatchInput::Message { input } if input == &accepted_input
    ));
    let rejected = message_queue
        .add(
            "s1",
            vec![TalosInputItem::Text {
                text: "too late".to_string(),
            }],
        )
        .unwrap();
    turn_executor
        .input_error
        .lock()
        .unwrap()
        .replace(TalosError::public(
            talos_error::ErrorCode::Conflict,
            "active turn ended before accepting the message".to_string(),
        ));

    let rejected_error = service
        .steer_queued_message(test_session(), &rejected.messages[0].id, "turn-active")
        .await
        .expect_err("rejected active-turn input must remain queued");

    assert!(rejected_error.is_code(talos_error::ErrorCode::Conflict));
    assert_eq!(message_queue.list("s1").unwrap(), rejected);
    message_queue
        .delete("s1", &rejected.messages[0].id)
        .unwrap();
    let skill_queue = message_queue
        .add(
            "s1",
            vec![TalosInputItem::SkillSelection {
                name: "review".to_string(),
                path: "/skills/review".to_string(),
            }],
        )
        .unwrap();
    let skill_id = skill_queue.messages[0].id.clone();

    let error = service
        .steer_queued_message(test_session(), &skill_id, "turn-active")
        .await
        .expect_err("skills are not supported by active turn steer");

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
    assert_eq!(
        message_queue.list("s1").unwrap().messages,
        skill_queue.messages
    );
}

#[tokio::test]
async fn submit_start_turn_records_completed_turn_progress() {
    let turn_executor = Arc::new(RecordingTurnExecutor::default());
    let (service, _event_service, store, _message_queue) =
        test_service(turn_executor.clone()).await;
    create_test_session_record(store.clone()).await;

    service
        .submit_start_turn(
            test_session(),
            StartTurnRequest {
                session_id: "s1".to_string(),
                input: vec![TalosInputItem::Text {
                    text: "hello".to_string(),
                }],
                context: None,
                model_selection: None,
                user_permission_mode: None,
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
    let (service, _event_service, store, _message_queue) = test_service(turn_executor).await;
    create_test_session_record(store.clone()).await;

    service
        .submit_start_turn(
            test_session(),
            StartTurnRequest {
                session_id: "s1".to_string(),
                input: vec![TalosInputItem::Text {
                    text: "hello".to_string(),
                }],
                context: None,
                model_selection: None,
                user_permission_mode: None,
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
    let (service, _event_service, store, _message_queue) =
        test_service(turn_executor.clone()).await;
    create_test_session_record(store.clone()).await;
    let request = StartTurnRequest {
        session_id: "s1".to_string(),
        input: vec![TalosInputItem::Text {
            text: "hello".to_string(),
        }],
        context: None,
        model_selection: None,
        user_permission_mode: None,
        project_id: None,
        current_directory: None,
        client_request_id: None,
    };
    let request_hash = turn_submission_hash("s1", &request.input).unwrap();
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
    let (service, _event_service, store, _message_queue) = test_service(turn_executor).await;
    create_test_session_record(store.clone()).await;

    service
        .submit_start_turn(
            test_session(),
            StartTurnRequest {
                session_id: "s1".to_string(),
                input: vec![TalosInputItem::Text {
                    text: "hello".to_string(),
                }],
                context: None,
                model_selection: None,
                user_permission_mode: None,
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
    let (service, _event_service, store, _message_queue) =
        test_service(turn_executor.clone()).await;
    create_test_session_record(store.clone()).await;
    insert_pending_dynamic_action(store.as_ref(), Some("tool-original")).await;

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
    let resume_context = request
        .resume_context
        .expect("resume context should be present");
    assert_eq!(resume_context.execution_metrics.tool_calls, 1);
    assert_eq!(resume_context.execution_metrics.model_rounds, 1);
    let tool_call = resume_context
        .tool_calls
        .into_iter()
        .next()
        .expect("resume tool call should be present");
    assert_eq!(tool_call.item_id, "tool-original");
    assert_eq!(tool_call.call_id, "call-1");
    assert_eq!(tool_call.tool_name, "AskUser");

    for _ in 0..20 {
        let active = store.get_active_turn("s1").await.unwrap().unwrap();
        if active.status == ActiveTurnStatus::Completed {
            assert_eq!(active.current_action_id, None);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let active = store.get_active_turn("s1").await.unwrap().unwrap();
    assert_eq!(active.status, ActiveTurnStatus::Completed);
    assert_eq!(active.current_action_id, None);
    let action = UserActionStore::get(store.as_ref(), "s1", "turn-original", "action-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(action.action.state, UserActionState::Resolved);
    let events = store
        .list_turn_events("s1", "turn-original", 32)
        .await
        .unwrap();
    let tool_result_index = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                AgentEventPayload::ItemDelta(payload)
                    if matches!(payload.delta, TalosTimelineDelta::ToolResult { .. })
            )
        })
        .expect("resumed tool result should be durable");
    let finished_index = events
        .iter()
        .position(|event| {
            matches!(
                &event.payload,
                AgentEventPayload::ItemDelta(payload)
                    if matches!(
                        payload.delta,
                        TalosTimelineDelta::UserAction {
                            event: TimelineUserActionEvent::Finished {
                                state: UserActionState::Resolved,
                                ..
                            }
                        }
                    )
            )
        })
        .expect("resolved user action fact should be durable");
    assert!(tool_result_index < finished_index);
}

#[tokio::test]
async fn sandbox_approval_cancel_finishes_the_turn_without_replaying_the_tool() {
    let turn_executor = Arc::new(RecordingTurnExecutor::default());
    let session_runtime = Arc::new(NoopSessionRuntime::default());
    let (service, _event_service, store, _message_queue) =
        test_service_with_runtime(turn_executor.clone(), session_runtime.clone()).await;
    create_test_session_record(store.clone()).await;
    insert_pending_sandbox_approval(store.as_ref()).await;

    let state = service
        .submit_user_action(
            test_session(),
            "turn-original",
            "action-1",
            UserActionResponse::SandboxApproval(SandboxApprovalDecision::Cancel),
        )
        .await
        .expect("cancel should finish the pending approval");

    assert_eq!(state, UserActionState::Cancelled);
    assert!(session_runtime.cancelled_turns.lock().unwrap().is_empty());
    assert!(turn_executor.dynamic_turn_id.lock().unwrap().is_none());
    assert!(turn_executor.stream_request.lock().unwrap().is_none());
    let record = UserActionStore::get(store.as_ref(), "s1", "turn-original", "action-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.action.state, UserActionState::Cancelled);
    assert_eq!(
        record.response,
        Some(UserActionResponse::SandboxApproval(
            SandboxApprovalDecision::Cancel
        ))
    );
    let active = store.get_active_turn("s1").await.unwrap().unwrap();
    assert_eq!(active.status, ActiveTurnStatus::Failed);
    assert_eq!(active.current_action_id, None);

    let retry_state = service
        .submit_user_action(
            test_session(),
            "turn-original",
            "action-1",
            UserActionResponse::SandboxApproval(SandboxApprovalDecision::Cancel),
        )
        .await
        .expect("same cancel response should be idempotent");
    assert_eq!(retry_state, UserActionState::Cancelled);
    assert!(session_runtime.cancelled_turns.lock().unwrap().is_empty());
}

#[tokio::test]
async fn submit_dynamic_tool_result_requires_original_tool_item_id() {
    let turn_executor = Arc::new(RecordingTurnExecutor::default());
    let (service, _event_service, store, _message_queue) =
        test_service(turn_executor.clone()).await;
    insert_pending_dynamic_action(store.as_ref(), None).await;

    let error = service
        .submit_dynamic_tool_result(test_session(), "call-1", "AskUser", "ok".to_string(), true)
        .await
        .expect_err("missing item id should fail before runtime dispatch");

    assert!(error.is_code(talos_error::ErrorCode::InternalError));
    assert!(turn_executor.stream_request.lock().unwrap().is_none());
    assert!(turn_executor.dynamic_turn_id.lock().unwrap().is_none());
    let active = store.get_active_turn("s1").await.unwrap().unwrap();
    assert_eq!(active.status, ActiveTurnStatus::WaitingUserAction);
    assert_eq!(active.current_action_id.as_deref(), Some("action-1"));
}

#[tokio::test]
async fn stale_user_action_submission_does_not_accept_the_response() {
    let turn_executor = Arc::new(RecordingTurnExecutor::default());
    let (service, _event_service, store, _message_queue) =
        test_service(turn_executor.clone()).await;
    insert_pending_dynamic_action(store.as_ref(), Some("tool-original")).await;
    store
        .upsert_active_turn(ActiveTurnUpdate {
            session_id: "s1".to_string(),
            turn_id: "turn-other".to_string(),
            message_id: Some("msg-other".to_string()),
            status: ActiveTurnStatus::WaitingUserAction,
            last_event_id: Some(TimelineEventId::from_seq(10)),
            current_action_id: Some("action-other".to_string()),
            updated_at: 2,
        })
        .await
        .unwrap();

    let error = service
        .submit_user_action(
            test_session(),
            "turn-original",
            "action-1",
            UserActionResponse::DynamicToolResult(DynamicToolResult {
                output: "ok".to_string(),
                success: true,
            }),
        )
        .await
        .expect_err("stale action should be rejected before mutation");

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    assert!(turn_executor.stream_request.lock().unwrap().is_none());
    let record = UserActionStore::get(store.as_ref(), "s1", "turn-original", "action-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.action.state, UserActionState::Pending);
    assert_eq!(record.response, None);
}

#[tokio::test]
async fn accepted_user_action_recovery_resumes_the_original_turn() {
    let turn_executor = Arc::new(RecordingTurnExecutor::default());
    let (service, event_service, store, _message_queue) = test_service(turn_executor.clone()).await;
    create_test_session_record(store.clone()).await;
    insert_pending_dynamic_action(store.as_ref(), Some("tool-original")).await;
    accept_pending_dynamic_action(&event_service).await;
    let active = store.get_active_turn("s1").await.unwrap().unwrap();

    service
        .recover_user_action(test_session(), active)
        .await
        .expect("accepted action should resume after recovery");

    assert_eq!(
        turn_executor.dynamic_turn_id.lock().unwrap().as_deref(),
        Some("turn-original")
    );
}

#[tokio::test]
async fn resuming_user_action_recovery_finishes_as_execution_unknown() {
    let turn_executor = Arc::new(RecordingTurnExecutor::default());
    let (service, event_service, store, _message_queue) = test_service(turn_executor.clone()).await;
    insert_pending_dynamic_action(store.as_ref(), Some("tool-original")).await;
    let accepted = accept_pending_dynamic_action(&event_service).await;
    event_service
        .user_actions()
        .begin_resume(accepted)
        .await
        .unwrap();
    let active = store.get_active_turn("s1").await.unwrap().unwrap();

    service
        .recover_user_action(test_session(), active)
        .await
        .expect("uncertain dispatch should become terminal");

    assert!(turn_executor.stream_request.lock().unwrap().is_none());
    let record = UserActionStore::get(store.as_ref(), "s1", "turn-original", "action-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.action.state, UserActionState::ExecutionUnknown);
    let active = store.get_active_turn("s1").await.unwrap().unwrap();
    assert_eq!(active.status, ActiveTurnStatus::Failed);
    assert_eq!(active.current_action_id, None);
    let events = store
        .list_turn_events("s1", "turn-original", 32)
        .await
        .unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        AgentEventPayload::ItemDelta(payload)
            if matches!(
                payload.delta,
                TalosTimelineDelta::UserAction {
                    event: TimelineUserActionEvent::Finished {
                        state: UserActionState::ExecutionUnknown,
                        ..
                    }
                }
            )
    )));
}

async fn accept_pending_dynamic_action(event_service: &TimelineEventService) -> UserActionRecord {
    event_service
        .user_actions()
        .accept(
            "s1",
            "turn-original",
            "action-1",
            UserActionResponse::DynamicToolResult(DynamicToolResult {
                output: "ok".to_string(),
                success: true,
            }),
        )
        .await
        .unwrap()
        .record
}
