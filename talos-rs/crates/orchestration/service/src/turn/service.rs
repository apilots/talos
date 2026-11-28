use agent_protocol::NodeAdmissionGates;
use std::{sync::Arc, time::Duration};

use talos_agent::{AgentInputParams, AgentRunParams, AgentRunSettlement};
use talos_agent_api::{
    ActiveTurnState, AgentEventPayload as CanonicalAgentEventPayload, AgentRunMetrics,
    AgentStopReason, AgentTurnRef, AgentUserContent, AgentUserInput,
};
use talos_error::{TalosError, TalosResult};
use talos_store::profile::{
    ModelCallUsageUpdate, ProfileStore, ProfileUsageUpdate, TurnUsageUpdate,
};
use talos_store::session::{
    CommandLease, CommandReservation, IdempotencyReservation, IdempotentCommand, SessionRecord,
};
use talos_types::{
    ModelRef, ModelUsageKind, SessionInfo, StartTurnRequest, StartTurnResponse, TalosConfig,
    TurnCommandStatus, UserActionResponse, UserActionState,
};
use tokio::sync::oneshot;
use {tracing::error, uuid::Uuid};

use crate::message_queue::MessageQueueReservation;
use crate::{InnerModelClient, SessionMessageQueueService};

use super::{
    auto_dispatch_session_from_event, retry_idempotency_key,
    start_response_from_idempotency_record, start_turn_hash, turn_submission_hash,
    validate_active_turn_input, validate_retry_request, validate_user_input, AgentExecutionDriver,
    PreparedTurnSubmission, StartTurnReservation, TurnRetryParams,
};

#[path = "../automation/goal_loop/turn.rs"]
mod automation_runtime;
mod completion;
mod destruction;
mod input;
mod interrupt;
mod managed_wake;
mod message_queue;

#[path = "service_subagent.rs"]
mod subagent;
#[path = "user_action_resume.rs"]
mod user_action_resume;

const TURN_SUBMISSION_LEASE_MS: i64 = 30_000;

#[derive(Clone)]
pub(crate) struct TurnService {
    pub(super) execution: AgentExecutionDriver,
    automation:
        Arc<std::sync::OnceLock<Arc<crate::automation::goal_loop::SessionAutomationService>>>,
    profile_store: Arc<dyn ProfileStore>,
    message_queue: SessionMessageQueueService,
    base_config: TalosConfig,
    execution_gate: Arc<NodeAdmissionGates>,
    nodes: Option<crate::ProductNodeAccess>,
    wake_dispatches:
        Arc<std::sync::Mutex<std::collections::HashSet<(String, managed_wake::WakeCandidate)>>>,
    session_owner: Arc<std::sync::RwLock<Option<talos_agent::SessionOwner>>>,
}

struct TurnSubmission {
    queue_message_id: Option<String>,
    origin: Option<talos_types::AutomationTurnOrigin>,
    input: AgentUserInput,
    retry_of_message_id: Option<String>,
}

impl TurnService {
    pub(crate) async fn drain_execution_delivery(&self) {
        self.execution.drain_delivery().await;
    }

    /// Assemble turn policy around the existing execution driver and its shared dependencies.
    pub fn new(
        execution: AgentExecutionDriver,
        profile_store: Arc<dyn ProfileStore>,
        message_queue: SessionMessageQueueService,
        base_config: TalosConfig,
        execution_gate: Arc<NodeAdmissionGates>,
    ) -> Self {
        Self {
            execution: execution.with_message_queue(message_queue.clone()),
            automation: Default::default(),
            profile_store,
            message_queue,
            base_config,
            execution_gate,
            nodes: None,
            wake_dispatches: Default::default(),
            session_owner: Default::default(),
        }
    }

    pub(crate) fn set_session_owner(&self, owner: talos_agent::SessionOwner) {
        *self.session_owner.write().expect("session owner lock") = Some(owner);
    }

    pub(crate) fn session_owner(&self) -> Option<talos_agent::SessionOwner> {
        self.session_owner
            .read()
            .expect("session owner lock")
            .clone()
    }

    pub(crate) fn claim_session_open(&self, session_id: &str) -> TalosResult<()> {
        if let Some(owner) = self.session_owner() {
            owner.claim_open(session_id)?;
        }
        Ok(())
    }

    pub(crate) fn pin_session(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<talos_agent::SessionLease>> {
        self.session_owner()
            .map(|owner| owner.pin(session_id))
            .transpose()
    }

    pub(crate) fn with_managed_workers(mut self, nodes: crate::ProductNodeAccess) -> Self {
        self.nodes = Some(nodes);
        self
    }

    pub(crate) fn with_inner_model(mut self, inner_model: Arc<InnerModelClient>) -> Self {
        self.execution = self.execution.with_inner_model(inner_model);
        self
    }

    pub(crate) async fn compact_session(
        &self,
        session: SessionRecord,
        request_id: String,
        identity: AgentTurnRef,
    ) -> TalosResult<SessionInfo> {
        let service = self.clone();
        await_admission(async move {
            service
                .compact_session_owned(session, request_id, identity)
                .await
        })
        .await
    }

    async fn compact_session_owned(
        &self,
        session: SessionRecord,
        request_id: String,
        identity: AgentTurnRef,
    ) -> TalosResult<SessionInfo> {
        let _admission_guard = self
            .execution_gate
            .for_node(&session.execution_node_id)
            .read_owned()
            .await;
        let context = self
            .execution
            .sessions
            .build_agent_context(&session, identity)
            .await?;
        let session_id = session.id.clone();
        let _request_id = request_id;
        self.execution.compact(context).await?;
        self.recover_worker_settlements(&session_id, false).await?;
        let record = self
            .execution
            .sessions
            .load_record(&session_id)
            .await?
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::SessionNotFound,
                    "Session is unavailable.",
                )
            })?;
        self.execution.sessions.session_info(&record).await
    }

    pub fn validate_start_turn_request(request: &StartTurnRequest) -> TalosResult<()> {
        validate_user_input(&request.input)
    }

    pub(crate) async fn reserve_start_turn(
        &self,
        request: &StartTurnRequest,
        idempotency_key: Option<&str>,
        wake_wait: Duration,
    ) -> TalosResult<StartTurnReservation> {
        let command = idempotency_key
            .map(|key| start_turn_hash(request).map(|hash| (key, hash)))
            .transpose()?;
        self.reserve_turn_submission(&request.session_id, command, wake_wait)
            .await
    }

    async fn reserve_turn_submission(
        &self,
        session_id: &str,
        command: Option<(&str, String)>,
        wake_wait: Duration,
    ) -> TalosResult<StartTurnReservation> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let prepared = PreparedTurnSubmission {
            request_id: format!("req_{}", Uuid::new_v4().simple()),
            turn_id: format!("turn_{}", Uuid::new_v4().simple()),
            message_id: format!("msg_{}", Uuid::new_v4().simple()),
            command: None,
        };
        let Some((key, request_hash)) = command else {
            return Ok(StartTurnReservation::Ready(prepared));
        };
        if key.trim().is_empty() {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Idempotency-Key must not be empty",
            ));
        }
        let reservation = self
            .execution
            .reserve_command(CommandReservation {
                session_id: session_id.to_owned(),
                command: IdempotentCommand::TurnSubmission,
                idempotency_key: key.to_string(),
                request_hash,
                request_id: prepared.request_id,
                turn_id: prepared.turn_id,
                message_id: prepared.message_id,
                lease_id: format!("lease_{}", Uuid::new_v4().simple()),
                now: now_ms,
                lease_expires_at: now_ms
                    .saturating_add(TURN_SUBMISSION_LEASE_MS + wake_wait.as_millis() as i64),
                record_expires_at: Some(now_ms.saturating_add(
                    (self.base_config.agent.command_idempotency_ttl_hours as i64) * 60 * 60 * 1000,
                )),
            })
            .await?;
        match reservation {
            IdempotencyReservation::Reserved(command) => {
                Ok(StartTurnReservation::Ready(PreparedTurnSubmission {
                    request_id: command.request_id.clone(),
                    turn_id: command.turn_id.clone(),
                    message_id: command.message_id.clone(),
                    command: Some(command),
                }))
            }
            IdempotencyReservation::Completed(record) => Ok(StartTurnReservation::Completed(
                start_response_from_idempotency_record(record)?,
            )),
            IdempotencyReservation::InProgress => Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "The idempotent Turn submission is still in progress",
            )),
        }
    }

    pub(crate) async fn release_start_turn(&self, prepared: PreparedTurnSubmission) {
        self.release_command(prepared.command).await;
    }

    pub async fn submit_start_turn(
        &self,
        session: SessionRecord,
        request: StartTurnRequest,
        prepared: PreparedTurnSubmission,
    ) -> TalosResult<StartTurnResponse> {
        Self::validate_start_turn_request(&request)?;
        self.submit_turn(
            session,
            TurnSubmission {
                origin: None,
                queue_message_id: None,
                input: request.input,
                retry_of_message_id: None,
            },
            None,
            Some(prepared),
            None,
        )
        .await
    }

    async fn submit_turn(
        &self,
        session: SessionRecord,
        submission: TurnSubmission,
        idempotency_key: Option<String>,
        prepared: Option<PreparedTurnSubmission>,
        auto_dispatch_ready: Option<oneshot::Receiver<()>>,
    ) -> TalosResult<StartTurnResponse> {
        let service = self.clone();
        await_admission(async move {
            service
                .submit_turn_owned(
                    session,
                    submission,
                    idempotency_key,
                    prepared,
                    auto_dispatch_ready,
                )
                .await
        })
        .await
    }

    async fn submit_turn_owned(
        &self,
        session: SessionRecord,
        mut submission: TurnSubmission,
        idempotency_key: Option<String>,
        mut prepared: Option<PreparedTurnSubmission>,
        auto_dispatch_ready: Option<oneshot::Receiver<()>>,
    ) -> TalosResult<StartTurnResponse> {
        // Recover before taking the gates used by terminal product settlement.
        // Keep the same file lock from recovery through new execution.
        let ownership = async {
            let ownership = self.pin_session(&session.id)?;
            if ownership.is_some() && submission.retry_of_message_id.is_none() {
                self.recover_if_orphan(&session).await?;
            }
            Ok(ownership)
        }
        .await;
        let ownership = match ownership {
            Ok(ownership) => ownership,
            Err(error) => {
                if let Some(prepared) = prepared.take() {
                    self.release_start_turn(prepared).await;
                }
                return Err(error);
            }
        };
        let automation = self.automation.get().cloned();
        let _automation_guard = if let Some(automation) = automation.as_ref() {
            Some(automation.lock(&session.id).await)
        } else {
            None
        };
        if let Some(automation) = automation.as_ref() {
            if let Some(origin) = submission.origin.as_ref() {
                if !self
                    .can_dispatch_automation(
                        &session.id,
                        &crate::automation::goal_loop::automation_object(origin),
                    )
                    .await?
                {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::Conflict,
                        "Automatic work must wait for an explicit request after cancellation.",
                    ));
                }
                // Recheck after asynchronous Skill resolution under the same owner gate
                // used by User enqueue. An unadmitted automatic turn never jumps User work.
                let waiting = self.message_queue.list(&session.id)?.messages;
                let human_waiting = waiting
                    .iter()
                    .any(|message| message.source == talos_types::QueuedMessageSource::User);
                if human_waiting
                    || (matches!(origin, talos_types::AutomationTurnOrigin::Goal { .. })
                        && !waiting.is_empty())
                {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::Conflict,
                        "Automatic work must wait for queued User messages.",
                    ));
                }
            }
            if submission.origin.is_none() {
                automation
                    .validate_user_input(
                        &session.id,
                        &submission.input,
                        submission.queue_message_id.as_deref(),
                    )
                    .await?;
                submission.input = automation
                    .normalize_activation(&session, submission.input)
                    .await?;
            }
        }
        let _admission_guard = self
            .execution_gate
            .for_node(&session.execution_node_id)
            .read_owned()
            .await;
        validate_user_input(&submission.input)?;
        let session_id = session.id.clone();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut request_id = format!("req_{}", Uuid::new_v4().simple());
        let mut turn_id = format!("turn_{}", Uuid::new_v4().simple());
        let mut message_id = submission
            .queue_message_id
            .clone()
            .unwrap_or_else(|| format!("msg_{}", Uuid::new_v4().simple()));
        let mut command_lease = None;

        let has_prepared = prepared.is_some();
        if let Some(prepared) = prepared {
            request_id = prepared.request_id;
            turn_id = prepared.turn_id;
            message_id = prepared.message_id;
            command_lease = prepared.command;
        }

        if let (false, Some(key)) = (has_prepared, idempotency_key.as_ref()) {
            let record_expires_at = now_ms.saturating_add(
                (self.base_config.agent.command_idempotency_ttl_hours as i64) * 60 * 60 * 1000,
            );
            let lease_expires_at = now_ms.saturating_add(TURN_SUBMISSION_LEASE_MS);
            match self
                .execution
                .reserve_command(CommandReservation {
                    session_id: session_id.clone(),
                    command: IdempotentCommand::TurnSubmission,
                    idempotency_key: key.clone(),
                    request_hash: turn_submission_hash(&session_id, &submission.input)?,
                    request_id: request_id.clone(),
                    turn_id: turn_id.clone(),
                    message_id: message_id.clone(),
                    lease_id: format!("lease_{}", Uuid::new_v4().simple()),
                    now: now_ms,
                    lease_expires_at,
                    record_expires_at: Some(record_expires_at),
                })
                .await?
            {
                IdempotencyReservation::Reserved(lease) => {
                    request_id = lease.request_id.clone();
                    turn_id = lease.turn_id.clone();
                    message_id = lease.message_id.clone();
                    command_lease = Some(lease);
                }
                IdempotencyReservation::Completed(record) => {
                    return start_response_from_idempotency_record(record);
                }
                IdempotencyReservation::InProgress => {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::Conflict,
                        "The idempotent Turn submission is still in progress",
                    ));
                }
            }
        }
        let validation = match submission.retry_of_message_id.as_deref() {
            Some(retry_of_message_id) => {
                self.validate_retry_attempt(&session_id, retry_of_message_id)
                    .await
            }
            None => self.reject_active_turn(&session_id).await,
        };
        if let Err(error) = validation {
            self.release_command(command_lease.take()).await;
            return Err(error);
        }

        let identity = AgentTurnRef::new(&session_id, &turn_id);
        let worktree_directive = if submission.origin.is_none()
            && submission.retry_of_message_id.is_none()
            && matches!(
                session.tree_identity,
                talos_store::session::SessionTreeIdentity::Primary
            ) {
            match crate::session::WorktreeDirective::from_input(&submission.input) {
                Ok(directive) => directive,
                Err(error) => {
                    self.release_command(command_lease.take()).await;
                    return Err(error);
                }
            }
        } else {
            None
        };
        let input = match super::completed_agent_user_input(
            &turn_id,
            &message_id,
            submission.input,
            command_lease.as_ref().map_or_else(
                || chrono::Utc::now().timestamp(),
                |command| command.created_at / 1_000,
            ),
        ) {
            Ok(input) => input,
            Err(error) => {
                self.release_command(command_lease.take()).await;
                return Err(error);
            }
        };
        let profile_context = self.profile_turn_context(&session, request_id.clone());

        let prepare = async {
            let sessions = &self.execution.sessions;
            let session = sessions.load_record(&session_id).await?.ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    "The Session no longer exists.",
                )
            })?;
            let worktree = worktree_directive.map(|directive| {
                let operation_id = match command_lease.as_ref() {
                    Some(lease) => {
                        use sha2::{Digest, Sha256};
                        let mut hash = Sha256::new();
                        hash.update((session_id.len() as u64).to_be_bytes());
                        hash.update(session_id.as_bytes());
                        hash.update(lease.idempotency_key.as_bytes());
                        let bytes = hash.finalize();
                        Uuid::from_bytes(bytes[..16].try_into().expect("SHA-256 digest length"))
                    }
                    None => Uuid::new_v4(),
                };
                (directive, operation_id)
            });
            let context = sessions
                .build_agent_context_for_turn(&session, identity.clone(), worktree)
                .await?;
            Ok(AgentRunParams {
                subagent_limit: None,
                command: None,
                automation_origin: None,
                context,
                input,
                checkpoint: None,
                continuation: None,
            })
        };
        let automation_object = submission
            .origin
            .as_ref()
            .map(crate::automation::goal_loop::automation_object);
        let subagent_limit = matches!(
            session.tree_identity,
            talos_store::session::SessionTreeIdentity::Subagent { .. }
        )
        .then_some(self.base_config.agent.subagent_admission.max_active_turns);
        let execution_result = self
            .execution
            .start(
                identity.clone(),
                prepare,
                command_lease.clone(),
                submission.origin,
                subagent_limit,
            )
            .await;
        let completion = match execution_result {
            Ok(completion) => completion,
            Err(error) => {
                if !agent_protocol::AgentAdmissionUnconfirmed::contains(&error) {
                    self.release_command(command_lease.take()).await;
                }
                return Err(error);
            }
        };
        if let Some(automation) = self.automation.get() {
            if let Some(object) = automation_object {
                automation.forget_explicit_dispatch(&session_id, &object);
            }
            automation.emit(&session_id);
        }
        self.spawn_turn_completion(
            AgentTurnRef::new(&session_id, &turn_id),
            profile_context,
            auto_dispatch_ready,
            completion,
            ownership,
        );

        Ok(StartTurnResponse {
            request_id,
            message_id: Some(message_id.clone()),
            session_id,
            turn_id,
            status: TurnCommandStatus::Accepted,
        })
    }

    pub(crate) async fn submit_retry(
        &self,
        actor: &crate::Actor,
        session: SessionRecord,
        request: TurnRetryParams,
    ) -> TalosResult<StartTurnResponse> {
        validate_retry_request(&request)?;
        validate_user_input(&request.input)?;
        let idempotency_key = retry_idempotency_key(&request);
        let mut prepared = None;
        if let Some(nodes) = &self.nodes {
            let wake_wait = nodes
                .managed_request_wait(&session.execution_node_id)
                .await?;
            if !wake_wait.is_zero() {
                let hash = turn_submission_hash(&session.id, &request.input)?;
                let reservation = self
                    .reserve_turn_submission(&session.id, Some((&idempotency_key, hash)), wake_wait)
                    .await?;
                let submission = match reservation {
                    StartTurnReservation::Ready(submission) => submission,
                    StartTurnReservation::Completed(response) => return Ok(response),
                };
                let readiness = async {
                    self.validate_retry_attempt(&session.id, &request.retry_of_message_id)
                        .await?;
                    nodes
                        .acquire_managed_worker(actor, &session.execution_node_id)
                        .await?;
                    Ok(())
                }
                .await;
                if let Err(error) = readiness {
                    self.release_start_turn(submission).await;
                    return Err(error);
                }
                prepared = Some(submission);
            }
        }
        self.submit_turn(
            session,
            TurnSubmission {
                origin: None,
                queue_message_id: None,
                input: request.input,
                retry_of_message_id: Some(request.retry_of_message_id),
            },
            Some(idempotency_key),
            prepared,
            None,
        )
        .await
    }

    async fn validate_retry_attempt(
        &self,
        session_id: &str,
        retry_of_message_id: &str,
    ) -> TalosResult<()> {
        if self.canonical_active_turn(session_id).await?.is_some() {
            return Err(TalosError::new(talos_error::ErrorCode::ActiveTurnRunning,
                "The original turn is still active; reconnect to the existing event stream instead."
                    .to_string(),
            ));
        }
        let terminal = self
            .execution
            .execution_store
            .get_agent_terminal_event_by_initial_message(session_id, retry_of_message_id)
            .await?
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::RetrySourceMismatch,
                    "retry_of_message_id does not identify the initial message of a terminal turn."
                        .to_string(),
                )
            })?;
        match terminal.payload {
            talos_agent_api::AgentEventPayload::TurnCompleted(_) => Err(TalosError::new(
                talos_error::ErrorCode::RetrySourceCompleted,
                "The original turn has already completed; refresh session history instead."
                    .to_string(),
            )),
            talos_agent_api::AgentEventPayload::TurnFailed(_) => Ok(()),
            talos_agent_api::AgentEventPayload::TurnStarted(_)
            | talos_agent_api::AgentEventPayload::MessageStarted(_)
            | talos_agent_api::AgentEventPayload::MessageBlockStarted(_)
            | talos_agent_api::AgentEventPayload::MessageBlockDelta(_)
            | talos_agent_api::AgentEventPayload::MessageBlockCompleted(_)
            | talos_agent_api::AgentEventPayload::MessageCompleted(_)
            | talos_agent_api::AgentEventPayload::WaitingForInput(_)
            | talos_agent_api::AgentEventPayload::MetricsUpdated(_) => Err(TalosError::internal(
                "terminal Agent event query returned a nonterminal fact",
            )),
        }
    }

    pub(crate) async fn recover_if_orphan(&self, session: &SessionRecord) -> TalosResult<()> {
        if let Some(active) = self.canonical_active_turn(&session.id).await? {
            if !self.is_turn_execution_active(&active.identity) {
                user_action_resume::recover_erased(self.clone(), session.clone(), active).await?;
            }
        }
        Ok(())
    }

    async fn reject_active_turn(&self, session_id: &str) -> TalosResult<()> {
        if self.canonical_active_turn(session_id).await?.is_none() {
            return Ok(());
        }
        Err(TalosError::new(talos_error::ErrorCode::ActiveTurnRunning,
            "A turn is already active for this session; reconnect to the existing event stream instead."
                .to_string(),
        ))
    }

    pub(crate) fn is_turn_execution_active(&self, identity: &AgentTurnRef) -> bool {
        self.execution.has_active_turn(identity)
    }

    pub(crate) async fn run_when_session_idle<T, F, Fut>(
        &self,
        session_id: &str,
        operation: F,
    ) -> TalosResult<Option<T>>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = TalosResult<T>>,
    {
        let _maintenance = match self.execution.begin_session_maintenance(session_id) {
            Ok(maintenance) => maintenance,
            Err(error) if error.is_code(talos_error::ErrorCode::ActiveTurnRunning) => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        if self.canonical_active_turn(session_id).await?.is_some() {
            return Ok(None);
        }
        operation().await.map(Some)
    }

    async fn release_command(&self, command: Option<CommandLease>) {
        let Some(command) = command else {
            return;
        };
        if let Err(error) = self
            .execution
            .execution_store
            .release_command(command)
            .await
        {
            error!(error = %error, "failed to release uncommitted idempotency lease");
        }
    }

    fn complete_resumed_turn(
        &self,
        session: SessionRecord,
        identity: AgentTurnRef,
        completion: oneshot::Receiver<TalosResult<AgentRunSettlement>>,
        ownership: Option<talos_agent::SessionLease>,
    ) {
        let request_id = format!("req_{}", Uuid::new_v4().simple());
        let profile_context = self.profile_turn_context(&session, request_id);
        self.spawn_turn_completion(identity, profile_context, None, completion, ownership);
    }
}

/// Keep Service admission and its completion handoff alive if the request is dropped.
/// This task owns only product submission; Harness continues to own execution cleanup.
pub(crate) async fn await_admission<T: Send + 'static>(
    admission: impl std::future::Future<Output = TalosResult<T>> + Send + 'static,
) -> TalosResult<T> {
    tokio::spawn(async move {
        let result = admission.await;
        if let Err(error) = &result {
            error!(error = %error, "Agent admission failed");
        }
        result
    })
    .await
    .map_err(|error| {
        TalosError::internal_with_source(
            "Agent admission task stopped; technical details were withheld.",
            error,
        )
    })?
}
