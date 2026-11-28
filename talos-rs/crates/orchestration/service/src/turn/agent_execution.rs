use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use talos_agent::{
    AgentAdmissionPermit, AgentContext, AgentInputParams, AgentRunExecution, AgentRunFailure,
    AgentRunParams, AgentRunSettlement,
};
use talos_agent_api::{
    AgentEventPayload, AgentInterruptOutcome, AgentMessage, AgentMessageBlockContent, AgentTurnRef,
    AgentUserContent,
};
use talos_coordinator::AgentCoordinator;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::session::{AgentExecutionStore, CommandLease};
use tokio::sync::{mpsc, oneshot, watch};

use crate::message_queue::MessageQueueReservation;
use crate::timeline::TimelineEventService;
use crate::SessionMessageQueueService;

mod execution_task;
mod notice;
mod timeline_writer;
mod worker_projection;

#[derive(Clone)]
struct ActiveTimelineStream {
    identity: AgentTurnRef,
    session: agent_protocol::AgentSessionContext,
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
        self.queue
            .acknowledge_admission(reservation.session_id(), reservation.message_id())
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

/// Product execution projection driver. The Worker commits execution facts
/// before admission returns; this driver materializes them for product policy.
#[derive(Clone)]
pub(crate) struct AgentExecutionDriver {
    // Shared only with the sibling turn policy; assembly owns their identity.
    pub(super) coordinator: Arc<dyn AgentCoordinator>,
    pub(super) execution_store: Arc<dyn AgentExecutionStore>,
    event_service: Arc<TimelineEventService>,
    pub(super) sessions: Arc<crate::session::SessionService>,
    active: Arc<Mutex<HashMap<String, LiveExecution>>>,
    delivery_tasks: tokio_util::task::TaskTracker,
    remote_observation_stop: tokio_util::sync::CancellationToken,
    message_queue: Option<SessionMessageQueueService>,
}

impl AgentExecutionDriver {
    pub(crate) fn begin_delivery_drain(&self) {
        self.remote_observation_stop.cancel();
        self.delivery_tasks.close();
    }

    pub(super) fn delivery_is_stopping(&self) -> bool {
        self.remote_observation_stop.is_cancelled()
    }

    fn ensure_accepting(&self) -> TalosResult<()> {
        if self.delivery_is_stopping() {
            return Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Product execution delivery is stopping.",
            ));
        }
        Ok(())
    }

    pub(super) fn track_admission(
        &self,
    ) -> TalosResult<tokio_util::task::task_tracker::TaskTrackerToken> {
        self.ensure_accepting()?;
        let admission = self.delivery_tasks.token();
        self.ensure_accepting()?;
        Ok(admission)
    }

    pub(crate) async fn drain_delivery(&self) {
        self.begin_delivery_drain();
        self.delivery_tasks.wait().await;
    }

    pub(super) fn observation_stop(
        &self,
        execution_node_id: &str,
    ) -> tokio_util::sync::CancellationToken {
        if execution_node_id == "embedded" {
            tokio_util::sync::CancellationToken::new()
        } else {
            self.remote_observation_stop.clone()
        }
    }

    pub(crate) fn spawn_delivery(
        &self,
        future: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        self.delivery_tasks.spawn(future);
    }

    pub(crate) fn has_live_execution(&self, session_id: &str) -> bool {
        self.active
            .lock()
            .expect("execution lock")
            .contains_key(session_id)
    }

    pub(super) async fn publish_active_turn(&self, session_id: &str) -> TalosResult<()> {
        self.event_service.publish_active_turn(session_id).await
    }
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
            sessions,
            active: Arc::new(Mutex::new(HashMap::new())),
            delivery_tasks: tokio_util::task::TaskTracker::new(),
            remote_observation_stop: tokio_util::sync::CancellationToken::new(),
            message_queue: None,
        }
    }

    pub(crate) fn with_message_queue(mut self, queue: SessionMessageQueueService) -> Self {
        self.message_queue = Some(queue);
        self
    }

    /// Reconcile an original in-flight identity before returning a duplicate reservation.
    /// Fresh submissions do not contact a sleeping Worker before the wake policy runs.
    pub(crate) async fn reserve_command(
        &self,
        params: talos_store::session::CommandReservation,
    ) -> TalosResult<talos_store::session::IdempotencyReservation> {
        let reservation = self.execution_store.reserve_command(params.clone()).await?;
        let reconcile = match &reservation {
            talos_store::session::IdempotencyReservation::InProgress => true,
            talos_store::session::IdempotencyReservation::Reserved(lease) => lease.reclaimed,
            talos_store::session::IdempotencyReservation::Completed(_) => false,
        };
        if !reconcile {
            return Ok(reservation);
        }
        let record = self
            .sessions
            .load_record(&params.session_id)
            .await?
            .ok_or_else(|| {
                TalosError::new(ErrorCode::SessionNotFound, "Session is unavailable.")
            })?;
        let scope = self.sessions.build_agent_session_context(&record).await?;
        self.project_worker(&scope).await?;
        let current = self.execution_store.reserve_command(params).await?;
        if matches!(
            reservation,
            talos_store::session::IdempotencyReservation::Reserved(_)
        ) && current == talos_store::session::IdempotencyReservation::InProgress
        {
            Ok(reservation)
        } else {
            Ok(current)
        }
    }

    pub(crate) async fn start(
        &self,
        identity: AgentTurnRef,
        prepare: impl std::future::Future<Output = TalosResult<AgentRunParams>>,
        command: Option<CommandLease>,
        automation_origin: Option<talos_types::AutomationTurnOrigin>,
        subagent_limit: Option<std::num::NonZeroU32>,
    ) -> TalosResult<oneshot::Receiver<TalosResult<AgentRunSettlement>>> {
        self.begin_admission(identity.clone()).await?;
        let mut params = match prepare.await {
            Ok(params) => params,
            Err(error) => {
                self.remove_live_execution(&identity);
                return Err(error);
            }
        };
        params.subagent_limit = subagent_limit;
        params.command = command.as_ref().map(command_identity);
        params.automation_origin = automation_origin.clone();
        if params.intent != agent_protocol::AgentRunIntent::Start {
            self.remove_live_execution(&identity);
            return Err(TalosError::internal(
                "fresh Agent execution requires a start intent",
            ));
        }
        let scope = params.context.session.clone();
        let execution = match self.coordinator.start(params).await {
            Ok(execution) => execution,
            Err(error) => {
                self.remove_live_execution(&identity);
                return Err(error);
            }
        };
        self.ingest_admission(execution, scope, ExecutionPurpose::Turn)
            .await
    }

    /// Run one Provider Session maintenance operation through the canonical
    /// Service commit and publication path.
    pub(crate) async fn compact(
        &self,
        context: AgentContext,
    ) -> TalosResult<talos_types::talos::TokenUsage> {
        let identity = context.identity();
        self.begin_admission(identity.clone()).await?;
        let execution = match self.coordinator.compact_session(context.clone()).await {
            Ok(execution) => execution,
            Err(error) => {
                self.remove_live_execution(&identity);
                return Err(error);
            }
        };
        let [_started] = execution.admission.events.as_slice() else {
            self.remove_live_execution(&identity);
            return Err(TalosError::internal(
                "Agent compaction admission must contain one TurnStarted event",
            ));
        };
        let completion = self
            .ingest_admission(
                execution,
                context.session.clone(),
                ExecutionPurpose::Compaction,
            )
            .await?;
        let settlement = completion.await.map_err(|error| {
            TalosError::internal_with_source(
                "Agent compaction writer stopped before reporting its result",
                error,
            )
        })??;
        match settlement {
            AgentRunSettlement::Finished {
                event,
                metrics,
                result,
                ..
            } => match event.payload {
                AgentEventPayload::TurnCompleted(_) | AgentEventPayload::TurnFailed(_) => {
                    result?;
                    Ok(metrics.usage)
                }
                _ => Err(TalosError::internal(
                    "Agent compaction settled with a nonterminal event",
                )),
            },
            AgentRunSettlement::WaitingForInput { .. } => Err(TalosError::internal(
                "Agent compaction cannot wait for input",
            )),
        }
    }

    /// Commit recovery only with an actual exact-Turn cleanup outcome from Coordinator.
    /// Callers own the live writer or Session exclusion; no Context is reconstructed.
    pub(crate) async fn settle_recovered_turn(
        &self,
        identity: AgentTurnRef,
        _expected_event_seq: i64,
        failure: AgentRunFailure,
        cleanup: AgentInterruptOutcome,
    ) -> TalosResult<AgentRunSettlement> {
        if let AgentInterruptOutcome::CleanupUnconfirmed { task_count } = cleanup {
            return Err(TalosError::new(
                ErrorCode::RunCleanupUnconfirmed,
                format!("Agent cleanup is unconfirmed for {task_count} task(s)"),
            ));
        }
        let original = failure.into_error();
        self.recovered_worker_settlement(&identity, &original).await
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
        self.ensure_accepting()?;
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
                TalosError::new(
                    ErrorCode::Conflict,
                    "message input targets no active Timeline writer",
                )
            })?;
        let mut params = params;
        params.command = command.as_ref().map(command_identity);
        let admission = self.coordinator.submit_input(params).await?;
        let (result_tx, result_rx) = oneshot::channel();
        active
            .input_tx
            .send(RunningInputCommit {
                permit: admission.permit,
                queued_input,
                result_tx,
            })
            .await
            .map_err(|_| {
                TalosError::new(
                    ErrorCode::Conflict,
                    "Agent Turn ended before input admission could be committed",
                )
            })?;
        result_rx.await.map_err(|_| {
            TalosError::new(
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
        user_action_response(&input).ok_or_else(|| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "waiting Agent continuation requires one UserAction response",
            )
        })?;
        self.begin_admission(identity.clone()).await?;
        let execution = match self
            .coordinator
            .start(AgentRunParams {
                subagent_limit: None,
                command: command.as_ref().map(command_identity),
                automation_origin: None,
                context: context.clone(),
                input,
                intent: agent_protocol::AgentRunIntent::RespondUserAction,
            })
            .await
        {
            Ok(execution) => execution,
            Err(error) => {
                self.remove_live_execution(&identity);
                return Err(error);
            }
        };
        self.ingest_admission(execution, context.session, ExecutionPurpose::Turn)
            .await
    }

    async fn ingest_admission(
        &self,
        execution: AgentRunExecution,
        session: agent_protocol::AgentSessionContext,
        purpose: ExecutionPurpose,
    ) -> TalosResult<oneshot::Receiver<TalosResult<AgentRunSettlement>>> {
        let identity = execution
            .admission
            .events
            .first()
            .ok_or_else(|| TalosError::internal("Worker admission is empty."))?
            .identity
            .clone();
        let result = async {
            let execution = commit_worker_admission(execution).await?;
            if let Err(error) = self.project_worker(&session).await {
                self.report_warning(&identity, error).await;
            }
            self.launch_execution(execution, session, purpose).await
        }
        .await;
        if result.is_err() {
            self.remove_live_execution(&identity);
        }
        result
    }

    async fn begin_admission(&self, identity: AgentTurnRef) -> TalosResult<()> {
        self.ensure_accepting()?;
        let previous = self
            .active
            .lock()
            .expect("active Timeline stream lock")
            .get(&identity.session_id)
            .and_then(|execution| match execution {
                LiveExecution::Running(stream) => Some(stream.identity.clone()),
                _ => None,
            });
        if let Some(previous) = previous {
            if self
                .execution_store
                .get_latest_agent_terminal_event(&identity.session_id)
                .await?
                .is_some_and(|event| event.identity == previous)
            {
                // A projected Worker terminal proves execution exited even while product
                // title/usage side effects are still completing on the previous writer.
                self.remove_live_execution(&previous);
            }
        }
        let mut active = self.active.lock().expect("active Timeline stream lock");
        if active.contains_key(&identity.session_id) {
            return Err(TalosError::new(
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
            return Err(TalosError::new(
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

fn command_identity(lease: &CommandLease) -> agent_protocol::AgentCommandIdentity {
    agent_protocol::AgentCommandIdentity {
        command: lease.command,
        idempotency_key: lease.idempotency_key.clone(),
        request_id: lease.request_id.clone(),
    }
}

async fn commit_worker_admission(
    mut execution: AgentRunExecution,
) -> TalosResult<AgentRunExecution> {
    let permit = std::mem::replace(
        &mut execution.admission.permit,
        AgentAdmissionPermit::new(async { Ok(()) }),
    );
    permit.consume().await?;
    Ok(execution)
}
