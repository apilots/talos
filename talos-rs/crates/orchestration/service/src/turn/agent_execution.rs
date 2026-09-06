use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use talos_agent::{
    AgentAdmissionPermit, AgentContext, AgentError, AgentEventReducer, AgentInputParams,
    AgentRunExecution, AgentRunParams, AgentRunSettlement,
};
use talos_agent_api::{
    AgentErrorInfo, AgentEvent, AgentEventPayload, AgentInterruptOutcome, AgentMessage,
    AgentMessageBlockContent, AgentMessageBlockStatus, AgentMessageStatus, AgentTurnFailed,
    AgentTurnRef, AgentUserContent,
};
use talos_coordinator::AgentCoordinator;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::session::{
    AgentEventCommit, AgentExecutionStore, ClaimContinuationParams, CommandLease,
    CommitInitialAdmissionParams, CommitTerminalSettlementParams,
};
use tokio::sync::{mpsc, oneshot, watch};

use crate::message_queue::MessageQueueReservation;
use crate::timeline::TimelineEventService;
use crate::user_workspace_memory::UserWorkspaceMemoryReflection;
use crate::SessionMessageQueueService;

mod timeline_writer;

#[derive(Clone)]
struct ActiveTimelineStream {
    identity: AgentTurnRef,
    agent_id: String,
    input_tx: mpsc::Sender<RunningInputCommit>,
    terminal_commit_tx: watch::Sender<bool>,
}

#[derive(Clone)]
enum LiveExecution {
    Admitting(AgentTurnRef),
    Running(Box<ActiveTimelineStream>),
    Maintenance { operation_id: String },
}

impl LiveExecution {
    fn matches_turn(&self, identity: &AgentTurnRef) -> bool {
        match self {
            Self::Admitting(active) => active == identity,
            Self::Running(active) => active.identity == *identity,
            Self::Maintenance { .. } => false,
        }
    }
}

pub(crate) struct SessionMaintenanceGuard {
    active: Arc<Mutex<HashMap<String, LiveExecution>>>,
    session_id: String,
    operation_id: String,
}

impl Drop for SessionMaintenanceGuard {
    fn drop(&mut self) {
        let mut active = self.active.lock().expect("active Timeline stream lock");
        if active.get(&self.session_id).is_some_and(|execution| {
            matches!(
                execution,
                LiveExecution::Maintenance { operation_id, .. }
                    if operation_id == &self.operation_id
            )
        }) {
            active.remove(&self.session_id);
        }
    }
}

struct RunningInputCommit {
    events: Vec<talos_agent_api::AgentEvent>,
    command: Option<CommandLease>,
    permit: AgentAdmissionPermit,
    queued_input: Option<QueuedRunningInput>,
    result_tx: oneshot::Sender<TalosResult<()>>,
}

struct QueuedRunningInput {
    queue: SessionMessageQueueService,
    reservation: Option<MessageQueueReservation>,
}

impl QueuedRunningInput {
    fn commit(mut self) -> TalosResult<()> {
        let reservation = self
            .reservation
            .take()
            .expect("queued running input reservation is consumed once");
        self.queue.commit(reservation).map(|_| ())
    }
}

impl Drop for QueuedRunningInput {
    fn drop(&mut self) {
        let Some(reservation) = self.reservation.take() else {
            return;
        };
        if let Err(error) = self.queue.release(reservation) {
            tracing::error!(error = %error, "failed to release queued running input reservation");
        }
    }
}

enum ExecutionPurpose {
    Turn,
    Compaction,
}

impl ExecutionPurpose {
    fn is_compaction(&self) -> bool {
        matches!(self, Self::Compaction)
    }
}

/// Service-owned canonical Agent execution driver.
///
/// It commits Agent facts before consuming Harness permits, then publishes the
/// exact committed records through the product Timeline.
#[derive(Clone)]
pub(crate) struct AgentExecutionDriver {
    coordinator: Arc<dyn AgentCoordinator>,
    execution_store: Arc<dyn AgentExecutionStore>,
    event_service: Arc<TimelineEventService>,
    memory_reflection: Option<Arc<UserWorkspaceMemoryReflection>>,
    sessions: Arc<crate::session::SessionService>,
    inner_model: Option<Arc<crate::InnerModelClient>>,
    active: Arc<Mutex<HashMap<String, LiveExecution>>>,
}

impl AgentExecutionDriver {
    pub(crate) fn new(
        coordinator: Arc<dyn AgentCoordinator>,
        execution_store: Arc<dyn AgentExecutionStore>,
        event_service: Arc<TimelineEventService>,
        sessions: Arc<crate::session::SessionService>,
    ) -> Self {
        Self {
            coordinator,
            execution_store,
            event_service,
            memory_reflection: None,
            sessions,
            inner_model: None,
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn with_inner_model(
        mut self,
        inner_model: Arc<crate::InnerModelClient>,
        memory_reflection: Arc<UserWorkspaceMemoryReflection>,
    ) -> Self {
        self.memory_reflection = Some(memory_reflection);
        self.inner_model = Some(inner_model);
        self
    }

    pub(crate) async fn start(
        &self,
        params: AgentRunParams,
        command: Option<CommandLease>,
    ) -> TalosResult<oneshot::Receiver<TalosResult<AgentRunSettlement>>> {
        if params.checkpoint.is_some() || params.continuation.is_some() {
            return Err(TalosError::internal(
                "fresh Agent execution cannot include continuation state",
            ));
        }
        let agent_id = params.context.session.agent_id.clone();
        let identity = params.context.identity();
        self.begin_admission(identity.clone())?;
        let execution = match self.coordinator.start(params).await {
            Ok(execution) => execution,
            Err(error) => {
                self.remove_live_execution(&identity);
                return Err(error);
            }
        };
        let commit = match self
            .execution_store
            .commit_initial_admission(CommitInitialAdmissionParams {
                events: execution.admission.events.clone(),
                command,
            })
            .await
        {
            Ok(commit) => commit,
            Err(error) => {
                self.remove_live_execution(&identity);
                return Err(error);
            }
        };
        self.launch_execution(execution, commit, agent_id, ExecutionPurpose::Turn)
            .await
    }

    /// Run one Provider Session maintenance operation through the canonical
    /// Service commit and publication path.
    pub(crate) async fn compact(
        &self,
        context: AgentContext,
    ) -> TalosResult<talos_types::talos::TokenUsage> {
        let identity = context.identity();
        self.begin_admission(identity.clone())?;
        let execution = match self.coordinator.compact_session(context.clone()).await {
            Ok(execution) => execution,
            Err(error) => {
                self.remove_live_execution(&identity);
                return Err(error);
            }
        };
        let [started] = execution.admission.events.as_slice() else {
            self.remove_live_execution(&identity);
            return Err(TalosError::internal(
                "Agent compaction admission must contain one TurnStarted event",
            ));
        };
        let committed = match self
            .execution_store
            .commit_maintenance_admission(started.clone())
            .await
        {
            Ok(commit) => commit,
            Err(error) => {
                self.remove_live_execution(&identity);
                return Err(error);
            }
        };
        let completion = self
            .launch_execution(
                execution,
                committed,
                context.session.agent_id,
                ExecutionPurpose::Compaction,
            )
            .await?;
        let settlement = completion.await.map_err(|_| {
            TalosError::internal("Agent compaction writer stopped before reporting its result")
        })??;
        match settlement {
            AgentRunSettlement::Finished { event, metrics, .. } => match event.payload {
                AgentEventPayload::TurnCompleted(_) => Ok(metrics.usage),
                AgentEventPayload::TurnFailed(failure) => Err(TalosError::public(
                    failure.error.code,
                    failure.error.message,
                )),
                _ => Err(TalosError::internal(
                    "Agent compaction settled with a nonterminal event",
                )),
            },
            AgentRunSettlement::WaitingForInput { .. } => Err(TalosError::internal(
                "Agent compaction cannot wait for input",
            )),
        }
    }

    async fn launch_execution(
        &self,
        execution: AgentRunExecution,
        commit: AgentEventCommit,
        agent_id: String,
        purpose: ExecutionPurpose,
    ) -> TalosResult<oneshot::Receiver<TalosResult<AgentRunSettlement>>> {
        let AgentRunExecution {
            admission,
            events,
            settlement,
        } = execution;
        let identity = commit.active_turn.identity.clone();
        let last_sequence = commit
            .events
            .last()
            .expect("initial Agent admission contains canonical events")
            .sequence;
        let durable_tail = commit.active_turn.last_event_seq;
        let (completion_tx, completion_rx) = oneshot::channel();
        let (input_tx, input_rx) = mpsc::channel(1);
        let (terminal_commit_tx, _terminal_commit_rx) = watch::channel(false);
        let active = ActiveTimelineStream {
            identity: identity.clone(),
            agent_id,
            input_tx,
            terminal_commit_tx,
        };
        self.promote_admission(active.clone())?;
        self.event_service.publish_committed(commit.events);
        let driver = self.clone();
        let task = tokio::spawn(async move {
            let result = match admission.permit.consume().await {
                Ok(()) => {
                    driver
                        .drive(
                            active,
                            events,
                            input_rx,
                            settlement,
                            (commit.active_turn.last_event_seq, last_sequence),
                            purpose,
                        )
                        .await
                }
                Err(error) => {
                    let error = error.into_talos_error();
                    tracing::error!(error = %error, "Agent start permit failed after durable admission");
                    driver
                        .recover_failed_execution(&active, durable_tail, &error)
                        .await
                }
            };
            driver.remove_live_execution(&identity);
            let _ = completion_tx.send(result);
        });
        drop(task);
        Ok(completion_rx)
    }

    /// Commit recovery only with an actual exact-Turn cleanup outcome from Coordinator.
    /// Callers own the live writer or Session exclusion; no Context is reconstructed.
    pub(crate) async fn settle_recovered_turn(
        &self,
        identity: AgentTurnRef,
        expected_event_seq: i64,
        failure: AgentErrorInfo,
        cleanup: AgentInterruptOutcome,
    ) -> TalosResult<AgentRunSettlement> {
        if let AgentInterruptOutcome::CleanupUnconfirmed { task_count } = cleanup {
            return Err(TalosError::public(
                ErrorCode::RunCleanupUnconfirmed,
                format!("Agent cleanup is unconfirmed for {task_count} task(s)"),
            ));
        }
        let checkpoint = self
            .event_service
            .turn_checkpoint(&identity, expected_event_seq)
            .await?;
        let mut reducer = AgentEventReducer::from_checkpoint(identity.clone(), checkpoint)
            .map_err(AgentError::into_talos_error)?;
        let cancelled = failure.code == ErrorCode::Cancelled;
        let occurred_at = chrono::Utc::now().timestamp();
        let recovery_events = reducer
            .close_open_lifecycle(
                if cancelled {
                    AgentMessageStatus::Cancelled
                } else {
                    AgentMessageStatus::Failed
                },
                if cancelled {
                    AgentMessageBlockStatus::Cancelled
                } else {
                    AgentMessageBlockStatus::Failed
                },
                occurred_at,
            )
            .map_err(AgentError::into_talos_error)?;
        let event = AgentEvent {
            identity,
            sequence: reducer.next_sequence(),
            occurred_at,
            payload: AgentEventPayload::TurnFailed(AgentTurnFailed { error: failure }),
        };
        reducer
            .apply(event.clone())
            .map_err(AgentError::into_talos_error)?;
        let mut current_event_seq = expected_event_seq;
        for recovery_event in recovery_events {
            let commit = self
                .execution_store
                .commit_progress_event(recovery_event, current_event_seq)
                .await?;
            current_event_seq = commit.active_turn.last_event_seq;
            self.event_service.publish_committed(commit.events);
        }
        let record = self
            .execution_store
            .commit_terminal_settlement(CommitTerminalSettlementParams {
                event: event.clone(),
                expected_event_seq: current_event_seq,
                completed_turn_metrics: None,
            })
            .await?;
        self.event_service.publish_committed([record]);
        let checkpoint = reducer.checkpoint();
        Ok(AgentRunSettlement::Finished {
            event,
            messages: checkpoint.messages,
            metrics: checkpoint.metrics,
        })
    }

    pub(crate) fn terminal_commit_waiter(
        &self,
        identity: &AgentTurnRef,
    ) -> Option<watch::Receiver<bool>> {
        self.active
            .lock()
            .expect("active Timeline stream lock")
            .get(&identity.session_id)
            .and_then(|execution| match execution {
                LiveExecution::Running(active) if active.identity == *identity => {
                    Some(active.terminal_commit_tx.subscribe())
                }
                LiveExecution::Admitting(_)
                | LiveExecution::Running(_)
                | LiveExecution::Maintenance { .. } => None,
            })
    }

    pub(crate) fn has_active_turn(&self, identity: &AgentTurnRef) -> bool {
        self.active
            .lock()
            .expect("active Timeline stream lock")
            .get(&identity.session_id)
            .is_some_and(|execution| execution.matches_turn(identity))
    }

    pub(crate) async fn wait_for_terminal_commit(
        mut waiter: watch::Receiver<bool>,
    ) -> TalosResult<()> {
        loop {
            if *waiter.borrow_and_update() {
                return Ok(());
            }
            waiter.changed().await.map_err(|_| {
                TalosError::internal(
                    "Agent Timeline writer stopped before terminal settlement became durable",
                )
            })?;
        }
    }

    /// Persist and schedule delivery of one direct running input.
    pub(crate) async fn admit_running_input(
        &self,
        params: AgentInputParams,
        command: Option<CommandLease>,
    ) -> TalosResult<()> {
        self.admit_running_input_inner(params, command, None).await
    }

    /// Persist one queued running input, consume its queue reservation, and schedule delivery.
    pub(crate) async fn admit_queued_running_input(
        &self,
        params: AgentInputParams,
        command: Option<CommandLease>,
        message_queue: SessionMessageQueueService,
        reservation: MessageQueueReservation,
    ) -> TalosResult<()> {
        self.admit_running_input_inner(
            params,
            command,
            Some(QueuedRunningInput {
                queue: message_queue,
                reservation: Some(reservation),
            }),
        )
        .await
    }

    async fn admit_running_input_inner(
        &self,
        params: AgentInputParams,
        command: Option<CommandLease>,
        queued_input: Option<QueuedRunningInput>,
    ) -> TalosResult<()> {
        let identity = params.identity.clone();
        let active = self
            .active
            .lock()
            .expect("active Timeline stream lock")
            .get(&identity.session_id)
            .and_then(|execution| match execution {
                LiveExecution::Running(active) if active.identity == identity => {
                    Some((**active).clone())
                }
                LiveExecution::Admitting(_)
                | LiveExecution::Running(_)
                | LiveExecution::Maintenance { .. } => None,
            })
            .ok_or_else(|| {
                TalosError::public(
                    ErrorCode::Conflict,
                    "message input targets no active Timeline writer",
                )
            })?;
        let admission = self.coordinator.submit_input(params).await?;
        let (result_tx, result_rx) = oneshot::channel();
        active
            .input_tx
            .send(RunningInputCommit {
                events: admission.events.clone(),
                command,
                permit: admission.permit,
                queued_input,
                result_tx,
            })
            .await
            .map_err(|_| {
                TalosError::public(
                    ErrorCode::Conflict,
                    "Agent Turn ended before input admission could be committed",
                )
            })?;
        result_rx.await.map_err(|_| {
            TalosError::public(
                ErrorCode::Conflict,
                "Agent Turn ended while input admission was being committed",
            )
        })??;
        Ok(())
    }

    pub(crate) async fn resume_waiting_input(
        &self,
        context: AgentContext,
        input: AgentMessage,
        command: Option<CommandLease>,
    ) -> TalosResult<oneshot::Receiver<TalosResult<AgentRunSettlement>>> {
        let identity = context.identity();
        let (action_id, _response) = user_action_response(&input).ok_or_else(|| {
            TalosError::public(
                ErrorCode::InvalidRequest,
                "waiting Agent continuation requires one UserAction response",
            )
        })?;
        let binding = self
            .execution_store
            .get_agent_continuation_binding(&identity)
            .await?
            .ok_or_else(|| {
                TalosError::public(ErrorCode::Conflict, "Agent continuation binding is missing")
            })?;
        let checkpoint = self
            .event_service
            .turn_checkpoint(&identity, binding.expected_event_seq)
            .await?;
        if action_id != binding.action_id {
            return Err(TalosError::public(
                ErrorCode::Conflict,
                "UserAction response does not match the durable continuation",
            ));
        }
        let message_id = input.message_id.clone();
        self.begin_admission(identity.clone())?;
        let execution = match self
            .coordinator
            .start(AgentRunParams {
                context: context.clone(),
                input,
                checkpoint: Some(checkpoint.clone()),
                continuation: Some(binding.provider_ref),
            })
            .await
        {
            Ok(execution) => execution,
            Err(error) => {
                self.remove_live_execution(&identity);
                return Err(error);
            }
        };
        let commit = match self
            .execution_store
            .claim_continuation(ClaimContinuationParams {
                events: execution.admission.events.clone(),
                action_id: binding.action_id.clone(),
                claim_id: message_id,
                expected_event_seq: binding.expected_event_seq,
                command,
            })
            .await
        {
            Ok(commit) => commit,
            Err(error) => {
                self.remove_live_execution(&identity);
                return Err(error);
            }
        };
        self.launch_execution(
            execution,
            commit,
            context.session.agent_id,
            ExecutionPurpose::Turn,
        )
        .await
    }

    fn begin_admission(&self, identity: AgentTurnRef) -> TalosResult<()> {
        let mut active = self.active.lock().expect("active Timeline stream lock");
        if active.contains_key(&identity.session_id) {
            return Err(TalosError::public(
                ErrorCode::ActiveTurnRunning,
                format!(
                    "session `{}` already has a live Agent execution",
                    identity.session_id
                ),
            ));
        }
        active.insert(
            identity.session_id.clone(),
            LiveExecution::Admitting(identity),
        );
        Ok(())
    }

    pub(crate) fn begin_session_maintenance(
        &self,
        session_id: &str,
    ) -> TalosResult<SessionMaintenanceGuard> {
        let mut active = self.active.lock().expect("active Timeline stream lock");
        if active.contains_key(session_id) {
            return Err(TalosError::public(
                ErrorCode::ActiveTurnRunning,
                format!("session `{session_id}` already has a live Agent execution"),
            ));
        }
        let operation_id = uuid::Uuid::new_v4().simple().to_string();
        active.insert(
            session_id.to_string(),
            LiveExecution::Maintenance {
                operation_id: operation_id.clone(),
            },
        );
        Ok(SessionMaintenanceGuard {
            active: self.active.clone(),
            session_id: session_id.to_string(),
            operation_id,
        })
    }

    fn promote_admission(&self, stream: ActiveTimelineStream) -> TalosResult<()> {
        let mut active = self.active.lock().expect("active Timeline stream lock");
        let Some(current) = active.get(&stream.identity.session_id) else {
            return Err(TalosError::internal(
                "live Agent admission owner is missing",
            ));
        };
        if !current.matches_turn(&stream.identity) {
            return Err(TalosError::internal(
                "live Agent admission owner changed before launch",
            ));
        }
        active.insert(
            stream.identity.session_id.clone(),
            LiveExecution::Running(Box::new(stream)),
        );
        Ok(())
    }

    fn remove_live_execution(&self, identity: &AgentTurnRef) {
        let mut active = self.active.lock().expect("active Timeline stream lock");
        if active
            .get(&identity.session_id)
            .is_some_and(|execution| execution.matches_turn(identity))
        {
            active.remove(&identity.session_id);
        }
    }
}

fn user_action_response(input: &AgentMessage) -> Option<(&str, &talos_types::UserActionResponse)> {
    if input.blocks.len() != 1 {
        return None;
    }
    let AgentMessageBlockContent::UserContent(AgentUserContent::UserActionResponse {
        action_id,
        response,
    }) = &input.blocks[0].content
    else {
        return None;
    };
    Some((action_id, response))
}
