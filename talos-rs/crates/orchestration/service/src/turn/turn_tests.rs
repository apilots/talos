use super::*;
use crate::test_support::{
    canonical_user_admission, commit_test_running_turn, run_test_migrations, test_user_input,
};
use crate::{SessionMessageQueueService, TimelineEventService};
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use talos_agent::{
    AgentAdmission, AgentAdmissionPermit, AgentContext, AgentInputParams, AgentInterruptParams,
    AgentProgressEventReceiver, AgentRunExecution, AgentRunParams, AgentRunSettlement,
    AgentRunSettlementReceiver, AgentSessionContext,
};
use talos_agent_api::{
    AgentContinuationRef, AgentEvent as CanonicalAgentEvent,
    AgentEventPayload as CanonicalAgentEventPayload, AgentInterruptOutcome, AgentMessage,
    AgentMessageBlock, AgentMessageBlockCompleted, AgentMessageBlockContent,
    AgentMessageBlockStarted, AgentMessageBlockStatus, AgentMessageRole, AgentMessageStarted,
    AgentMessageStatus, AgentRunMetrics, AgentStopReason, AgentToolCall, AgentToolCallArguments,
    AgentToolResult, AgentTurnCompleted, AgentTurnFailed, AgentUserContent, AgentUserInput,
};
use talos_coordinator::{AgentCoordinator, AgentSelectionParams};
use talos_storage::SqliteStorage;
use talos_store::profile::{DatabaseProfileStore, ProfileStore};
use talos_store::project::MockProjectStore;
use talos_store::session::{
    AgentExecutionStore, CommandReservation, DatabaseSessionStore, IdempotentCommand,
    SessionRecord, SessionStore, SessionTreeIdentity,
};
use talos_types::talos::{SessionSourceProvider, SessionSourceType, StopReason};
use talos_types::{
    DynamicToolResult, DynamicToolResultParams, ModelRef, ModelSelection, PendingUserAction,
    SandboxApprovalDecision, SandboxApprovalParams, SandboxApprovalPresentation,
    SandboxApprovalReason, SessionConf, StartTurnRequest, TalosConfig, UserActionParams,
    UserActionResponse, UserActionState,
};
use tokio::sync::{mpsc, Notify};

#[path = "queue_start_tests.rs"]
mod queue_start_tests;

#[test]
fn turn_input_accepts_files_without_text() {
    assert!(validate_user_input(&AgentUserInput {
        content: vec![AgentUserContent::File {
            file_id: "file-1".to_string(),
        }],
    })
    .is_ok());
}

#[path = "idempotency_behavior_tests.rs"]
mod idempotency_behavior_tests;
#[path = "service/interrupt_tests.rs"]
mod interrupt_lifecycle_tests;
#[path = "queue_dispatch_tests.rs"]
mod queue_dispatch_tests;
#[path = "retry_tests.rs"]
mod retry_tests;
#[path = "running_input_tests.rs"]
mod running_input_tests;
#[path = "turn_progress_tests.rs"]
mod turn_progress_tests;
#[path = "user_action_recovery_tests.rs"]
mod user_action_recovery_tests;
#[path = "user_action_turn_tests.rs"]
mod user_action_turn_tests;

#[test]
fn turn_input_rejects_duplicate_file_references() {
    assert!(validate_user_input(&AgentUserInput {
        content: vec![
            AgentUserContent::File {
                file_id: "file-1".to_string(),
            },
            AgentUserContent::File {
                file_id: "file-1".to_string(),
            },
        ],
    })
    .is_err());
}

async fn submit_test_start(
    service: &TurnService,
    session: SessionRecord,
    request: StartTurnRequest,
    idempotency_key: Option<&str>,
) -> TalosResult<StartTurnResponse> {
    match service
        .reserve_start_turn(&request, idempotency_key)
        .await?
    {
        StartTurnReservation::Ready(prepared) => {
            service.submit_start_turn(session, request, prepared).await
        }
        StartTurnReservation::Completed(response) => Ok(response),
    }
}

struct RecordingAgentExecution {
    stream_request: Mutex<Option<AgentRunParams>>,
    stream_requests: Mutex<Vec<AgentRunParams>>,
    stream_request_received: Notify,
    stream_error: Mutex<Option<TalosError>>,
    input_error: Mutex<Option<TalosError>>,
    input_permit_error: Mutex<Option<talos_error::TalosError>>,
    start_permit_error: Mutex<Option<talos_error::TalosError>>,
    panic_start_permit: AtomicBool,
    progress_event: Mutex<Option<CanonicalAgentEventPayload>>,
    hold_input_permit: Mutex<bool>,
    input_permit_started: Arc<Notify>,
    input_permit_release: Arc<Notify>,
    input_permit_completed: Arc<AtomicBool>,
    submitted_inputs: Mutex<Vec<AgentInputParams>>,
    turn_errors_remaining: Mutex<usize>,
    dynamic_turn_id: Mutex<Option<String>>,
    completed_stop_reason: Mutex<Option<StopReason>>,
    hold_running_turn: Mutex<bool>,
    hold_admission: AtomicBool,
    admission_reached: Notify,
    admission_release: Notify,
    active_progress: Mutex<Option<mpsc::Sender<CanonicalAgentEvent>>>,
    active_settlement: Mutex<
        Option<
            tokio::sync::oneshot::Sender<talos_agent::AgentResult<talos_agent::AgentRunSettlement>>,
        >,
    >,
}

impl Default for RecordingAgentExecution {
    fn default() -> Self {
        Self {
            stream_request: Mutex::new(None),
            stream_requests: Mutex::new(Vec::new()),
            stream_request_received: Notify::new(),
            stream_error: Mutex::new(None),
            input_error: Mutex::new(None),
            input_permit_error: Mutex::new(None),
            start_permit_error: Mutex::new(None),
            panic_start_permit: AtomicBool::new(false),
            progress_event: Mutex::new(None),
            hold_input_permit: Mutex::new(false),
            input_permit_started: Arc::new(Notify::new()),
            input_permit_release: Arc::new(Notify::new()),
            input_permit_completed: Arc::new(AtomicBool::new(false)),
            submitted_inputs: Mutex::new(Vec::new()),
            turn_errors_remaining: Mutex::new(0),
            dynamic_turn_id: Mutex::new(None),
            completed_stop_reason: Mutex::new(None),
            hold_running_turn: Mutex::new(false),
            hold_admission: AtomicBool::new(false),
            admission_reached: Notify::new(),
            admission_release: Notify::new(),
            active_progress: Mutex::new(None),
            active_settlement: Mutex::new(None),
        }
    }
}

struct RecordingExecutionCoordinator {
    execution: Arc<RecordingAgentExecution>,
    delegate: Arc<dyn AgentCoordinator>,
}

#[derive(Default)]
struct RecordingAgentCoordinator {
    cancelled_turns: Mutex<Vec<String>>,
    interrupt_error: Mutex<Option<TalosError>>,
}

#[async_trait]
impl AgentCoordinator for RecordingAgentCoordinator {
    async fn resolve_agent(&self, _params: AgentSelectionParams) -> TalosResult<String> {
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

    async fn start(&self, _params: AgentRunParams) -> TalosResult<AgentRunExecution> {
        unreachable!("execution is delegated by the scenario Coordinator")
    }

    async fn submit_input(&self, _params: AgentInputParams) -> TalosResult<AgentAdmission> {
        unreachable!("input is delegated by the scenario Coordinator")
    }

    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome> {
        self.cancelled_turns
            .lock()
            .unwrap()
            .push(params.identity.turn_id);
        if let Some(error) = self.interrupt_error.lock().unwrap().take() {
            return Err(error);
        }
        Ok(AgentInterruptOutcome::Cancelled)
    }

    async fn compact_session(&self, _params: AgentContext) -> TalosResult<AgentRunExecution> {
        unreachable!("not used in turn service tests")
    }

    async fn clear_session(
        &self,
        _params: AgentSessionContext,
        _expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn close_session(
        &self,
        _params: AgentSessionContext,
        _expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn shutdown(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }
}

#[async_trait]
impl AgentCoordinator for RecordingExecutionCoordinator {
    async fn resolve_agent(&self, params: AgentSelectionParams) -> TalosResult<String> {
        self.delegate.resolve_agent(params).await
    }

    async fn ensure_agent(&self, agent_id: &str) -> TalosResult<()> {
        self.delegate.ensure_agent(agent_id).await
    }

    async fn select_execution_node(
        &self,
        agent_id: &str,
        user_id: &str,
        preferred_node_id: Option<&str>,
    ) -> TalosResult<String> {
        self.delegate
            .select_execution_node(agent_id, user_id, preferred_node_id)
            .await
    }

    async fn start(&self, request: AgentRunParams) -> TalosResult<AgentRunExecution> {
        if request.input.blocks.iter().any(|block| {
            matches!(
                block.content,
                talos_agent_api::AgentMessageBlockContent::UserContent(
                    talos_agent_api::AgentUserContent::UserActionResponse { .. }
                )
            )
        }) {
            self.execution
                .dynamic_turn_id
                .lock()
                .unwrap()
                .replace(request.context.turn_id.clone());
        }
        self.execution
            .stream_request
            .lock()
            .unwrap()
            .replace(request.clone());
        self.execution
            .stream_requests
            .lock()
            .unwrap()
            .push(request.clone());
        self.execution.stream_request_received.notify_one();
        if let Some(error) = self.execution.stream_error.lock().unwrap().take() {
            return Err(error);
        }
        if self.execution.hold_admission.load(Ordering::SeqCst) {
            self.execution.admission_reached.notify_one();
            self.execution.admission_release.notified().await;
        }
        let identity = request.context.identity();
        let is_continuation = request.checkpoint.is_some();
        let start_sequence = request
            .checkpoint
            .as_ref()
            .map_or(1, |checkpoint| checkpoint.last_sequence.saturating_add(1));
        let admission = canonical_user_admission(
            identity.clone(),
            request.input.clone(),
            start_sequence,
            !is_continuation,
        );
        let mut next_sequence = admission
            .last()
            .expect("canonical test admission")
            .sequence
            .saturating_add(1);
        let mut progress = Vec::new();
        if let Some(checkpoint) = request.checkpoint.as_ref() {
            let (original_message, original_block) = checkpoint
                .messages
                .iter()
                .find_map(|message| {
                    message
                        .blocks
                        .iter()
                        .find(|block| block.status == AgentMessageBlockStatus::WaitingForInput)
                        .map(|block| (message.clone(), block.clone()))
                })
                .expect("continued test Turn should contain a waiting block");
            let tool_result = AgentToolResult {
                metrics: None,
                call_id: "call-1".to_string(),
                tool_name: "AskUser".to_string(),
                content: vec![talos_types::talos::TalosContentItem::Text {
                    text: "resumed".to_string(),
                }],
                is_error: false,
            };
            let result_message_id = "tool-result-message".to_string();
            let started_result_message = AgentMessage {
                message_id: result_message_id.clone(),
                turn_id: identity.turn_id.clone(),
                role: AgentMessageRole::Tool,
                status: AgentMessageStatus::InProgress,
                blocks: Vec::new(),
                created_at: 4,
                completed_at: None,
            };
            let started_result_block = AgentMessageBlock {
                block_id: "tool-result-block".to_string(),
                status: AgentMessageBlockStatus::Running,
                content: AgentMessageBlockContent::ToolResult(tool_result.clone()),
                started_at: 4,
                completed_at: None,
            };
            let completed_result_block = AgentMessageBlock {
                status: AgentMessageBlockStatus::Completed,
                completed_at: Some(5),
                ..started_result_block.clone()
            };
            let completed_result_message = AgentMessage {
                status: AgentMessageStatus::Completed,
                blocks: vec![completed_result_block.clone()],
                completed_at: Some(5),
                ..started_result_message.clone()
            };
            progress.push(CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: next_sequence,
                occurred_at: 4,
                payload: CanonicalAgentEventPayload::MessageStarted(AgentMessageStarted {
                    message: started_result_message,
                }),
            });
            next_sequence += 1;
            progress.push(CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: next_sequence,
                occurred_at: 4,
                payload: CanonicalAgentEventPayload::MessageBlockStarted(
                    AgentMessageBlockStarted {
                        message_id: result_message_id.clone(),
                        block: started_result_block,
                    },
                ),
            });
            next_sequence += 1;
            progress.push(CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: next_sequence,
                occurred_at: 5,
                payload: CanonicalAgentEventPayload::MessageBlockCompleted(
                    AgentMessageBlockCompleted {
                        message_id: result_message_id,
                        block: completed_result_block,
                    },
                ),
            });
            next_sequence += 1;
            progress.push(CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: next_sequence,
                occurred_at: 5,
                payload: CanonicalAgentEventPayload::MessageCompleted(completed_result_message),
            });
            next_sequence += 1;
            let completed_original_block = AgentMessageBlock {
                status: AgentMessageBlockStatus::Completed,
                completed_at: Some(6),
                ..original_block
            };
            progress.push(CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: next_sequence,
                occurred_at: 6,
                payload: CanonicalAgentEventPayload::MessageBlockCompleted(
                    AgentMessageBlockCompleted {
                        message_id: original_message.message_id.clone(),
                        block: completed_original_block.clone(),
                    },
                ),
            });
            next_sequence += 1;
            progress.push(CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: next_sequence,
                occurred_at: 6,
                payload: CanonicalAgentEventPayload::MessageCompleted(AgentMessage {
                    status: AgentMessageStatus::Completed,
                    blocks: vec![completed_original_block],
                    completed_at: Some(6),
                    ..original_message
                }),
            });
            next_sequence += 1;
        }
        if let Some(payload) = self.execution.progress_event.lock().unwrap().take() {
            progress.push(CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: next_sequence,
                occurred_at: 1,
                payload,
            });
            next_sequence += 1;
        }
        let should_error = {
            let mut turn_errors_remaining = self.execution.turn_errors_remaining.lock().unwrap();
            let should_error = *turn_errors_remaining > 0;
            *turn_errors_remaining = turn_errors_remaining.saturating_sub(1);
            should_error
        };
        let terminal = if should_error {
            CanonicalAgentEvent {
                identity,
                sequence: next_sequence,
                occurred_at: 1,
                payload: CanonicalAgentEventPayload::TurnFailed(AgentTurnFailed {
                    error: talos_error::ApiError {
                        code: talos_error::ErrorCode::InternalError,
                        message: "injected Turn failure".to_string(),
                        localization: None,
                    },
                    retryable: false,
                }),
            }
        } else {
            let stop_reason = self
                .execution
                .completed_stop_reason
                .lock()
                .unwrap()
                .take()
                .unwrap_or(StopReason::EndTurn);
            if stop_reason == StopReason::Cancelled {
                CanonicalAgentEvent {
                    identity,
                    sequence: next_sequence,
                    occurred_at: 1,
                    payload: CanonicalAgentEventPayload::TurnFailed(AgentTurnFailed {
                        error: talos_error::ApiError {
                            code: talos_error::ErrorCode::Cancelled,
                            message: "injected cancellation".to_string(),
                            localization: None,
                        },
                        retryable: false,
                    }),
                }
            } else {
                CanonicalAgentEvent {
                    identity,
                    sequence: next_sequence,
                    occurred_at: 1,
                    payload: CanonicalAgentEventPayload::TurnCompleted(AgentTurnCompleted {
                        stop_reason: AgentStopReason::EndTurn,
                    }),
                }
            }
        };
        let result = match &terminal.payload {
            CanonicalAgentEventPayload::TurnFailed(failed) => Err(TalosError::new(
                failed.error.code,
                failed.error.message.clone(),
            )),
            _ => Ok(()),
        };
        let (progress_tx, progress_rx) = mpsc::channel(progress.len().max(1));
        let (settlement_tx, settlement_rx) = tokio::sync::oneshot::channel();
        let mut settlement_tx = Some(settlement_tx);
        let hold_running_turn = *self.execution.hold_running_turn.lock().unwrap();
        let start_permit_error = self.execution.start_permit_error.lock().unwrap().take();
        let panic_start_permit = self
            .execution
            .panic_start_permit
            .swap(false, Ordering::SeqCst);
        if hold_running_turn {
            self.execution
                .active_progress
                .lock()
                .unwrap()
                .replace(progress_tx.clone());
            self.execution
                .active_settlement
                .lock()
                .unwrap()
                .replace(settlement_tx.take().expect("test settlement sender"));
        }
        Ok(AgentRunExecution {
            admission: AgentAdmission {
                events: admission,
                permit: AgentAdmissionPermit::new(async move {
                    assert!(!panic_start_permit, "injected start permit panic");
                    if let Some(error) = start_permit_error {
                        return Err(error);
                    }
                    for event in progress {
                        progress_tx.send(event).await.map_err(|_| {
                            talos_error::TalosError::new(
                                talos_error::ErrorCode::InternalError,
                                "test progress receiver",
                            )
                        })?;
                    }
                    if hold_running_turn {
                        return Ok(());
                    }
                    drop(progress_tx);
                    settlement_tx
                        .expect("completed test Turn settlement sender")
                        .send(Ok(AgentRunSettlement::Finished {
                            result,
                            event: terminal,
                            messages: Vec::new(),
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
    }

    async fn submit_input(&self, params: AgentInputParams) -> TalosResult<AgentAdmission> {
        if let Some(error) = self.execution.input_error.lock().unwrap().take() {
            return Err(error);
        }
        if params.input.blocks.iter().any(|block| {
            matches!(
                block.content,
                talos_agent_api::AgentMessageBlockContent::UserContent(
                    talos_agent_api::AgentUserContent::UserActionResponse { .. }
                )
            )
        }) {
            self.execution
                .dynamic_turn_id
                .lock()
                .unwrap()
                .replace(params.identity.turn_id.clone());
        }
        let sequence = 6;
        let events = canonical_user_admission(
            params.identity.clone(),
            params.input.clone(),
            sequence,
            false,
        );
        self.execution.submitted_inputs.lock().unwrap().push(params);
        let permit_error = self.execution.input_permit_error.lock().unwrap().take();
        let hold_permit = *self.execution.hold_input_permit.lock().unwrap();
        let permit_started = self.execution.input_permit_started.clone();
        let permit_release = self.execution.input_permit_release.clone();
        let permit_completed = self.execution.input_permit_completed.clone();
        Ok(AgentAdmission {
            events,
            permit: AgentAdmissionPermit::new(async move {
                permit_started.notify_one();
                if hold_permit {
                    permit_release.notified().await;
                }
                permit_completed.store(true, Ordering::SeqCst);
                permit_error.map_or(Ok(()), Err)
            }),
        })
    }

    async fn interrupt(&self, params: AgentInterruptParams) -> TalosResult<AgentInterruptOutcome> {
        self.delegate.interrupt(params).await
    }

    async fn compact_session(&self, params: AgentContext) -> TalosResult<AgentRunExecution> {
        self.delegate.compact_session(params).await
    }

    async fn clear_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        self.delegate
            .clear_session(params, expected_history_cursor)
            .await
    }

    async fn close_session(
        &self,
        params: AgentSessionContext,
        expected_history_cursor: talos_types::SessionHistoryCursor,
    ) -> TalosResult<()> {
        self.delegate
            .close_session(params, expected_history_cursor)
            .await
    }

    async fn shutdown(&self, timeout: Duration) -> TalosResult<()> {
        self.delegate.shutdown(timeout).await
    }
}

#[tokio::test]
async fn submit_start_turn_preserves_executor_error_semantics() {
    let agent_execution = Arc::new(RecordingAgentExecution {
        stream_error: Mutex::new(Some(TalosError::new(
            talos_error::ErrorCode::Conflict,
            "agent busy".to_string(),
        ))),
        ..Default::default()
    });
    let (service, _event_service, store, _message_queue) = test_service(agent_execution).await;
    create_test_session_record(store.clone()).await;

    let error = submit_test_start(
        &service,
        test_session(),
        StartTurnRequest {
            node_id: None,
            session_id: "s1".to_string(),
            input: test_user_input("hello"),
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
    agent_execution: Arc<RecordingAgentExecution>,
) -> (
    TurnService,
    Arc<TimelineEventService>,
    Arc<DatabaseSessionStore>,
    SessionMessageQueueService,
) {
    test_service_with_coordinator(
        agent_execution,
        Arc::new(RecordingAgentCoordinator::default()),
    )
    .await
}

async fn test_service_with_coordinator(
    agent_execution: Arc<RecordingAgentExecution>,
    coordinator: Arc<dyn AgentCoordinator>,
) -> (
    TurnService,
    Arc<TimelineEventService>,
    Arc<DatabaseSessionStore>,
    SessionMessageQueueService,
) {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    test_service_with_storage(agent_execution, coordinator, storage).await
}

async fn test_service_with_storage(
    agent_execution: Arc<RecordingAgentExecution>,
    coordinator: Arc<dyn AgentCoordinator>,
    storage: Arc<SqliteStorage>,
) -> (
    TurnService,
    Arc<TimelineEventService>,
    Arc<DatabaseSessionStore>,
    SessionMessageQueueService,
) {
    run_test_migrations(storage.pool()).await;
    let profile_store: Arc<dyn ProfileStore> =
        Arc::new(DatabaseProfileStore::new(storage.pool().clone()));
    let store = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
    let event_service = Arc::new(TimelineEventService::new(store.clone()));
    let config = test_config();
    let sessions = Arc::new(crate::test_support::session_service(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config.clone(),
    ));
    let message_queue =
        SessionMessageQueueService::new(10).expect("queue service should initialize");
    let coordinator: Arc<dyn AgentCoordinator> = Arc::new(RecordingExecutionCoordinator {
        execution: agent_execution,
        delegate: coordinator,
    });
    (
        TurnService::new(
            AgentExecutionDriver::new(coordinator, store.clone(), event_service.clone(), sessions),
            profile_store,
            message_queue.clone(),
            config,
            Arc::new(crate::turn::NodeAdmissionGates::default()),
        ),
        event_service,
        store,
        message_queue,
    )
}

async fn insert_pending_dynamic_action(store: &DatabaseSessionStore, item_id: Option<&str>) {
    let pending_action = PendingUserAction {
        action_id: "action-1".to_string(),
        call_id: "call-1".to_string(),
        item_id: item_id.map(str::to_string),
        state: UserActionState::Pending,
        params: UserActionParams::DynamicToolResult(DynamicToolResultParams {
            tool_name: "AskUser".to_string(),
        }),
    };
    insert_canonical_pending_action(
        store,
        pending_action,
        item_id.unwrap_or("tool-original"),
        "AskUser",
    )
    .await;
}

async fn insert_canonical_pending_action(
    store: &DatabaseSessionStore,
    pending_action: PendingUserAction,
    item_id: &str,
    tool_name: &str,
) {
    let identity = talos_agent_api::AgentTurnRef::new("s1", "turn-original");
    let running_message = completed_agent_user_input(
        "turn-original",
        "msg-original",
        test_user_input("running"),
        1,
    )
    .unwrap();
    let mut commit = commit_test_running_turn(store, identity.clone(), running_message).await;
    let assistant_message = AgentMessage {
        message_id: "agent-message-original".to_string(),
        turn_id: identity.turn_id.clone(),
        role: AgentMessageRole::Agent,
        status: AgentMessageStatus::InProgress,
        blocks: Vec::new(),
        created_at: 2,
        completed_at: None,
    };
    commit = store
        .commit_progress_event(
            CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: commit.events.last().unwrap().sequence + 1,
                occurred_at: 2,
                payload: CanonicalAgentEventPayload::MessageStarted(AgentMessageStarted {
                    message: assistant_message,
                }),
            },
            commit.active_turn.last_event_seq,
        )
        .await
        .unwrap();
    let tool_block = AgentMessageBlock {
        block_id: item_id.to_string(),
        status: AgentMessageBlockStatus::Running,
        content: AgentMessageBlockContent::ToolCall(AgentToolCall {
            call_id: "call-1".to_string(),
            name: tool_name.to_string(),
            arguments: AgentToolCallArguments::Streaming {
                json_fragment: "{}".to_string(),
            },
            progress: Vec::new(),
        }),
        started_at: 3,
        completed_at: None,
    };
    commit = store
        .commit_progress_event(
            CanonicalAgentEvent {
                identity: identity.clone(),
                sequence: commit.events.last().unwrap().sequence + 1,
                occurred_at: 3,
                payload: CanonicalAgentEventPayload::MessageBlockStarted(
                    AgentMessageBlockStarted {
                        message_id: "agent-message-original".to_string(),
                        block: tool_block,
                    },
                ),
            },
            commit.active_turn.last_event_seq,
        )
        .await
        .unwrap();
    let waiting = CanonicalAgentEvent {
        identity,
        sequence: commit
            .events
            .last()
            .expect("canonical admission event")
            .sequence
            .saturating_add(1),
        occurred_at: 2,
        payload: CanonicalAgentEventPayload::WaitingForInput(
            talos_agent_api::AgentPendingInput::UserAction {
                action: pending_action,
            },
        ),
    };
    store
        .commit_waiting_settlement(
            waiting,
            AgentContinuationRef {
                opaque_id: "continuation-1".to_string(),
            },
            commit.active_turn.last_event_seq,
        )
        .await
        .unwrap();
}

async fn insert_pending_sandbox_approval(store: &DatabaseSessionStore) {
    let pending_action = PendingUserAction {
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
    };
    insert_canonical_pending_action(store, pending_action, "tool-original", "Bash").await;
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
        reasoning_replay: Default::default(),
        reasoning_efforts: None,
        request_parameters: Default::default(),
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

fn test_session() -> SessionRecord {
    SessionRecord {
        id: "s1".to_string(),
        agent_id: "default".to_string(),
        execution_node_id: "embedded".to_string(),
        tree_identity: SessionTreeIdentity::Primary,
        workspace_id: "default".to_string(),
        title: "Untitled Session".to_string(),
        user_id: "user-1".to_string(),
        source_type: SessionSourceType::Http,
        source_provider: SessionSourceProvider::Http,
        channel_id: None,
        thread_id: None,
        environment: None,
        created_at: 0,
        last_activity: 0,
        message_count: 0,
        latest_turn_seq: 0,
        memory_processed_until_event_seq: 0,
        usage: Default::default(),
        conf: SessionConf {
            model_selection: Some(test_model_selection()),
            ..SessionConf::default()
        },
        project_id: None,
        current_directory: None,
    }
}

async fn create_test_session_record(store: Arc<DatabaseSessionStore>) {
    let record = test_session();
    store
        .insert_session_record(&record)
        .await
        .expect("session record should be created");
}

async fn wait_for_stream_requests(executor: &RecordingAgentExecution, expected: usize) {
    // Synchronize with admission, not polling speed or cold stack symbolization.
    // The timeout is only a deadlock guard for a stalled test fixture.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if executor.stream_requests.lock().unwrap().len() >= expected {
                return;
            }
            executor.stream_request_received.notified().await;
        }
    })
    .await
    .expect("queued turns should be dispatched");
}

async fn wait_for_canonical_terminal(store: &DatabaseSessionStore) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if store.get_agent_active_turn("s1").await.unwrap().is_none() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("canonical Turn should settle");
}
