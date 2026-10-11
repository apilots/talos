use std::sync::Arc;

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

mod completion;
mod input;
mod interrupt;
mod message_queue;

#[path = "service_subagent.rs"]
mod subagent;
#[path = "user_action_resume.rs"]
mod user_action_resume;

const TURN_SUBMISSION_LEASE_MS: i64 = 30_000;

#[derive(Clone)]
pub(crate) struct TurnService {
    execution: AgentExecutionDriver,
    profile_store: Arc<dyn ProfileStore>,
    message_queue: SessionMessageQueueService,
    base_config: TalosConfig,
    execution_gate: Arc<super::NodeAdmissionGates>,
}

struct TurnSubmission {
    input: AgentUserInput,
    retry_of_message_id: Option<String>,
}

impl TurnService {
    /// Assemble turn policy around the existing execution driver and its shared dependencies.
    pub fn new(
        execution: AgentExecutionDriver,
        profile_store: Arc<dyn ProfileStore>,
        message_queue: SessionMessageQueueService,
        base_config: TalosConfig,
        execution_gate: Arc<super::NodeAdmissionGates>,
    ) -> Self {
        Self {
            execution,
            profile_store,
            message_queue,
            base_config,
            execution_gate,
        }
    }

    pub(crate) fn with_user_workspace_memory_reflection(
        mut self,
        inner_model: Arc<InnerModelClient>,
    ) -> Self {
        let sessions = self.execution.sessions.clone();
        let coordinator = self.execution.coordinator.clone();
        self.execution = self.execution.with_inner_model(
            inner_model.clone(),
            Arc::new(
                crate::user_workspace_memory::UserWorkspaceMemoryReflection::new(
                    inner_model,
                    coordinator,
                    sessions,
                ),
            ),
        );
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
        let usage = self.execution.compact(context).await?;
        let completed_at = chrono::Utc::now();
        let timezone_name = self.execution.sessions.kernel_timezone();
        let timezone = talos_config::parse_timezone(&timezone_name)?;
        let model_ref = session
            .conf
            .model_selection
            .as_ref()
            .expect("loaded Session must have a model selection")
            .model_ref
            .clone();
        self.profile_store
            .record_usage(ProfileUsageUpdate::ModelCall(ModelCallUsageUpdate {
                user_id: session.user_id.clone(),
                session_id: Some(session.id.clone()),
                request_id,
                kind: ModelUsageKind::Compact,
                model_ref,
                usage: usage.clone(),
                usage_available: usage.is_available(),
                timezone: timezone_name,
                completed_at_ms: completed_at.timestamp_millis(),
                usage_date: completed_at
                    .with_timezone(&timezone)
                    .date_naive()
                    .format("%Y-%m-%d")
                    .to_string(),
            }))
            .await?;
        let record = self
            .execution
            .sessions
            .record_compaction_usage(session, &usage)
            .await?;
        self.execution.sessions.session_info(&record)
    }

    pub fn validate_start_turn_request(request: &StartTurnRequest) -> TalosResult<()> {
        validate_user_input(&request.input)
    }

    pub(crate) async fn reserve_start_turn(
        &self,
        request: &StartTurnRequest,
        idempotency_key: Option<&str>,
    ) -> TalosResult<StartTurnReservation> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let prepared = PreparedTurnSubmission {
            request_id: format!("req_{}", Uuid::new_v4().simple()),
            turn_id: format!("turn_{}", Uuid::new_v4().simple()),
            message_id: format!("msg_{}", Uuid::new_v4().simple()),
            command: None,
        };
        let Some(key) = idempotency_key else {
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
            .execution_store
            .reserve_command(CommandReservation {
                session_id: request.session_id.clone(),
                command: IdempotentCommand::TurnSubmission,
                idempotency_key: key.to_string(),
                request_hash: start_turn_hash(request)?,
                request_id: prepared.request_id,
                turn_id: prepared.turn_id,
                message_id: prepared.message_id,
                lease_id: format!("lease_{}", Uuid::new_v4().simple()),
                now: now_ms,
                lease_expires_at: now_ms.saturating_add(TURN_SUBMISSION_LEASE_MS),
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
        submission: TurnSubmission,
        idempotency_key: Option<String>,
        prepared: Option<PreparedTurnSubmission>,
        auto_dispatch_ready: Option<oneshot::Receiver<()>>,
    ) -> TalosResult<StartTurnResponse> {
        let _admission_guard = self
            .execution_gate
            .for_node(&session.execution_node_id)
            .read_owned()
            .await;
        validate_user_input(&submission.input)?;
        let session_id = session.id.clone();
        let request_hash = turn_submission_hash(&session_id, &submission.input)?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut request_id = format!("req_{}", Uuid::new_v4().simple());
        let mut turn_id = format!("turn_{}", Uuid::new_v4().simple());
        let mut message_id = format!("msg_{}", Uuid::new_v4().simple());
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
                .execution_store
                .reserve_command(CommandReservation {
                    session_id: session_id.clone(),
                    command: IdempotentCommand::TurnSubmission,
                    idempotency_key: key.clone(),
                    request_hash: request_hash.clone(),
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
        let context = match self
            .execution
            .sessions
            .build_agent_context(&session, identity.clone())
            .await
        {
            Ok(context) => context,
            Err(error) => {
                self.release_command(command_lease.take()).await;
                return Err(error);
            }
        };
        let input = match super::completed_agent_user_input(
            &turn_id,
            &message_id,
            submission.input,
            chrono::Utc::now().timestamp(),
        ) {
            Ok(input) => input,
            Err(error) => {
                self.release_command(command_lease.take()).await;
                return Err(error);
            }
        };
        let profile_context = self.profile_turn_context(&session, request_id.clone());

        let completion = match self
            .execution
            .start(
                AgentRunParams {
                    context,
                    input,
                    checkpoint: None,
                    continuation: None,
                },
                command_lease.clone(),
            )
            .await
        {
            Ok(completion) => completion,
            Err(error) => {
                self.release_command(command_lease.take()).await;
                return Err(error);
            }
        };
        self.spawn_turn_completion(
            AgentTurnRef::new(&session_id, &turn_id),
            profile_context,
            auto_dispatch_ready,
            completion,
        );

        Ok(StartTurnResponse {
            request_id,
            message_id: Some(message_id.clone()),
            session_id,
            turn_id,
            status: TurnCommandStatus::Accepted,
        })
    }

    pub async fn submit_retry(
        &self,
        session: SessionRecord,
        request: TurnRetryParams,
    ) -> TalosResult<StartTurnResponse> {
        validate_retry_request(&request)?;
        validate_user_input(&request.input)?;
        let idempotency_key = retry_idempotency_key(&request);
        self.submit_turn(
            session,
            TurnSubmission {
                input: request.input,
                retry_of_message_id: Some(request.retry_of_message_id),
            },
            Some(idempotency_key),
            None,
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
    ) {
        let request_id = format!("req_{}", Uuid::new_v4().simple());
        let profile_context = self.profile_turn_context(&session, request_id);
        self.spawn_turn_completion(identity, profile_context, None, completion);
    }
}

/// Keep Service admission and its completion handoff alive if the request is dropped.
/// This task owns only product submission; Harness continues to own execution cleanup.
async fn await_admission<T: Send + 'static>(
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
