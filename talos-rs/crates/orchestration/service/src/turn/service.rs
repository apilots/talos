use std::sync::Arc;
use std::time::Duration;

use talos_coordinator::SessionRuntime;
use talos_error::{TalosError, TalosResult};
use talos_store::session::{
    ActiveTurnStatus, IdempotencyReservation, IdempotencyStatus, MessageCommandResult,
    MessageIdempotencyStore, PendingUserAction,
};
use talos_types::agent_dispatch::{
    AgentDispatchInput, AgentDispatchInputParams, AgentDispatchParams, AgentDispatchResumeContext,
    AgentDispatchResumeToolCall,
};
use talos_types::talos::{AgentEvent, AgentEventPayload, TalosInputItem, TalosTurnErrorEvent};
use talos_types::{StartTurnRequest, TalosConfig, TalosSession};
use tokio::sync::mpsc;
use tracing::error;
use uuid::Uuid;

use crate::session::SessionService;
use crate::timeline::TimelineEventService;

use super::{
    completed_turn_from_event, record_completed_turn_progress, result_from_idempotency_record,
    retry_idempotency_key, spawn_event_forwarder, turn_submission_hash, validate_retry_request,
    validate_text_input, TurnExecutor, TurnRetryParams,
};

#[derive(Clone)]
pub struct TurnService {
    executor: Arc<dyn TurnExecutor>,
    event_service: Arc<TimelineEventService>,
    idempotency_store: Arc<dyn MessageIdempotencyStore>,
    sessions: Arc<SessionService>,
    session_runtime: Arc<dyn SessionRuntime>,
    base_config: TalosConfig,
}

struct TurnSubmission {
    input: Vec<TalosInputItem>,
    workspace_id: Option<String>,
}

struct ActiveResumeContext {
    turn_id: String,
    message_id: String,
    gateway_resume: Option<AgentDispatchResumeContext>,
}

impl TurnService {
    pub fn new(
        executor: Arc<dyn TurnExecutor>,
        event_service: Arc<TimelineEventService>,
        idempotency_store: Arc<dyn MessageIdempotencyStore>,
        sessions: Arc<SessionService>,
        session_runtime: Arc<dyn SessionRuntime>,
        base_config: TalosConfig,
    ) -> Self {
        Self {
            executor,
            event_service,
            idempotency_store,
            sessions,
            session_runtime,
            base_config,
        }
    }

    fn session_for_turn_dispatch(
        session: &TalosSession,
        workspace_id_override: Option<String>,
    ) -> TalosSession {
        let mut dispatch_session = session.clone();
        if let Some(workspace_id) = workspace_id_override {
            dispatch_session.context.workspace_id = Some(workspace_id);
        }
        dispatch_session
    }

    fn session_for_resume_dispatch(session: &TalosSession, message_id: String) -> TalosSession {
        let mut dispatch_session = session.clone();
        dispatch_session.context.message_id = message_id;
        dispatch_session
    }

    pub fn validate_start_turn_request(request: &StartTurnRequest) -> TalosResult<()> {
        validate_text_input(&request.input)
    }

    pub async fn submit_start_turn(
        &self,
        session: TalosSession,
        request: StartTurnRequest,
        idempotency_key: Option<String>,
    ) -> TalosResult<MessageCommandResult> {
        Self::validate_start_turn_request(&request)?;
        self.submit_turn(
            session,
            TurnSubmission {
                input: request.input,
                workspace_id: request.workspace_id,
            },
            idempotency_key,
        )
        .await
    }

    async fn submit_turn(
        &self,
        session: TalosSession,
        submission: TurnSubmission,
        idempotency_key: Option<String>,
    ) -> TalosResult<MessageCommandResult> {
        validate_text_input(&submission.input)?;
        let session_id = session.context.session_id.clone();
        let request_hash =
            turn_submission_hash(&session_id, &submission.input, &submission.workspace_id)?;
        let now_ms = chrono::Utc::now().timestamp_millis();
        let request_id = format!("req_{}", Uuid::new_v4().simple());
        let turn_id = format!("turn_{}", Uuid::new_v4().simple());

        if let Some(key) = idempotency_key.as_ref() {
            let expires_at = now_ms.saturating_add(
                (self
                    .base_config
                    .agent
                    .kernel
                    .timeline_events
                    .idempotency_key_ttl_hours as i64)
                    * 60
                    * 60
                    * 1000,
            );
            match self
                .idempotency_store
                .reserve(
                    &session_id,
                    key,
                    &request_hash,
                    &turn_id,
                    now_ms,
                    Some(expires_at),
                )
                .await
                .map_err(|err| TalosError::IdempotencyError(err.to_string()))?
            {
                IdempotencyReservation::Reserved(_) => {}
                IdempotencyReservation::Existing(record) => {
                    if record.request_hash != request_hash {
                        return Err(TalosError::IdempotencyConflict(
                            "Idempotency-Key was already used with a different request".to_string(),
                        ));
                    }
                    return result_from_idempotency_record(record);
                }
            }
        }
        self.reject_active_turn(&session_id).await?;

        let dispatch_request = AgentDispatchParams {
            turn_id: turn_id.clone(),
            session: Self::session_for_turn_dispatch(&session, submission.workspace_id.clone()),
            resume_context: None,
            input: submission.input.clone(),
        };
        let assistant_model = session.config.llm.model.clone();

        let mut stream_rx = self.executor.start_turn_stream(dispatch_request).await?;
        let first_event_timeout = Duration::from_millis(
            self.base_config
                .agent
                .kernel
                .timeline_events
                .first_event_timeout_ms
                .max(1),
        );
        let first_event = match tokio::time::timeout(first_event_timeout, stream_rx.recv()).await {
            Ok(Some(event)) => event,
            Ok(None) => {
                return self
                    .fail_without_first_event(
                        &session_id,
                        &turn_id,
                        &request_id,
                        idempotency_key.as_deref(),
                        &assistant_model,
                        stream_rx,
                        "message stream closed before first event",
                    )
                    .await;
            }
            Err(_) => {
                if let Err(err) = self
                    .session_runtime
                    .cancel_active_turn(talos_types::CancelActiveTurnParams {
                        session: session.clone(),
                        turn_id: turn_id.clone(),
                    })
                    .await
                {
                    error!(
                        session_id = %session_id,
                        turn_id = %turn_id,
                        error = %err,
                        "failed to cancel active turn after first event timeout"
                    );
                }
                return self
                    .fail_without_first_event(
                        &session_id,
                        &turn_id,
                        &request_id,
                        idempotency_key.as_deref(),
                        &assistant_model,
                        stream_rx,
                        "timed out waiting for first timeline event",
                    )
                    .await;
            }
        };
        let message_id = first_event.message_id.clone();
        let progress_event = first_event.clone();
        self.event_service
            .record_talos_event(first_event)
            .await
            .map_err(|err| TalosError::StorageError(format!("event record failed: {err}")))?;
        self.record_completed_turn_progress(&progress_event, &assistant_model)
            .await;
        spawn_event_forwarder(
            self.event_service.clone(),
            self.executor.clone(),
            self.sessions.clone(),
            self.session_runtime.clone(),
            assistant_model,
            completed_turn_from_event(&progress_event),
            stream_rx,
        );

        let result = MessageCommandResult {
            request_id,
            message_id: Some(message_id.clone()),
            session_id: session_id.clone(),
            turn_id,
            status: IdempotencyStatus::Accepted,
        };
        if let Some(key) = idempotency_key.as_ref() {
            self.idempotency_store
                .complete(
                    &session_id,
                    key,
                    IdempotencyStatus::Accepted,
                    Some(&message_id),
                    &result,
                    chrono::Utc::now().timestamp_millis(),
                )
                .await
                .map_err(|err| TalosError::IdempotencyError(err.to_string()))?;
        }

        Ok(result)
    }

    pub async fn submit_dynamic_tool_result(
        &self,
        session: TalosSession,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        let session_id = session.context.session_id.clone();
        let resume = self
            .pending_dynamic_resume_context(&session_id, call_id, tool_name)
            .await?;
        let turn_id = resume.turn_id.clone();
        self.executor
            .submit_input(AgentDispatchInputParams {
                session: session.clone(),
                turn_id,
                input: AgentDispatchInput::DynamicToolResult {
                    call_id: call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    output,
                    success,
                },
            })
            .await?;
        self.event_service
            .resolve_pending_user_action(&session_id)
            .await
            .map_err(|err| TalosError::StorageError(format!("active turn update failed: {err}")))?;
        self.resume_after_user_action(session, resume).await
    }

    pub async fn submit_retry(
        &self,
        session: TalosSession,
        request: TurnRetryParams,
    ) -> TalosResult<MessageCommandResult> {
        validate_retry_request(&request)?;
        validate_text_input(&request.input)?;
        let session_id = session.context.session_id.clone();
        let request_hash =
            turn_submission_hash(&session_id, &request.input, &request.workspace_id)?;
        let idempotency_key = retry_idempotency_key(&request);
        if let Some(record) = self
            .idempotency_store
            .get(&session_id, &idempotency_key)
            .await
            .map_err(|err| TalosError::IdempotencyError(err.to_string()))?
        {
            if record.request_hash != request_hash {
                return Err(TalosError::IdempotencyConflict(
                    "Retry attempt was already used with a different request".to_string(),
                ));
            }
            return result_from_idempotency_record(record);
        }

        self.validate_retry_attempt(&session_id, &request).await?;
        self.submit_turn(
            session,
            TurnSubmission {
                input: request.input,
                workspace_id: request.workspace_id,
            },
            Some(idempotency_key),
        )
        .await
    }

    async fn validate_retry_attempt(
        &self,
        session_id: &str,
        request: &TurnRetryParams,
    ) -> TalosResult<()> {
        let Some(active_turn) = self.event_service.get_active_turn(session_id).await? else {
            return Ok(());
        };

        match active_turn.status {
            ActiveTurnStatus::Running
            | ActiveTurnStatus::WaitingUserAction
            | ActiveTurnStatus::Cancelling => Err(TalosError::RetryActiveTurnRunning(
                "The original turn is still active; reconnect to the existing event stream instead."
                    .to_string(),
            )),
            ActiveTurnStatus::Completed => Err(TalosError::RetrySourceCompleted(
                "The original turn has already completed; refresh session history instead."
                    .to_string(),
            )),
            ActiveTurnStatus::Failed => {
                if active_turn.message_id.as_deref() == Some(request.retry_of_message_id.as_str()) {
                    Ok(())
                } else {
                    Err(TalosError::RetrySourceMismatch(
                        "retry_of_message_id does not match the failed active turn.".to_string(),
                    ))
                }
            }
        }
    }

    async fn reject_active_turn(&self, session_id: &str) -> TalosResult<()> {
        let Some(active_turn) = self.event_service.get_active_turn(session_id).await? else {
            return Ok(());
        };
        match active_turn.status {
            ActiveTurnStatus::Running
            | ActiveTurnStatus::WaitingUserAction
            | ActiveTurnStatus::Cancelling => Err(TalosError::ActiveTurnRunning(
                "A turn is already active for this session; reconnect to the existing event stream instead."
                    .to_string(),
            )),
            ActiveTurnStatus::Completed | ActiveTurnStatus::Failed => Ok(()),
        }
    }

    async fn resume_after_user_action(
        &self,
        session: TalosSession,
        resume: ActiveResumeContext,
    ) -> TalosResult<()> {
        let session_id = session.context.session_id.clone();
        let request_id = format!("req_{}", Uuid::new_v4().simple());
        let turn_id = resume.turn_id;
        let dispatch_request = AgentDispatchParams {
            turn_id: turn_id.clone(),
            session: Self::session_for_resume_dispatch(&session, resume.message_id),
            resume_context: resume.gateway_resume,
            input: Vec::new(),
        };
        let assistant_model = session.config.llm.model.clone();
        let mut stream_rx = self.executor.start_turn_stream(dispatch_request).await?;
        let first_event_timeout = Duration::from_millis(
            self.base_config
                .agent
                .kernel
                .timeline_events
                .first_event_timeout_ms
                .max(1),
        );
        let first_event = match tokio::time::timeout(first_event_timeout, stream_rx.recv()).await {
            Ok(Some(event)) => event,
            Ok(None) => {
                let _ = self
                    .fail_without_first_event(
                        &session_id,
                        &turn_id,
                        &request_id,
                        None,
                        &assistant_model,
                        stream_rx,
                        "message stream closed before first event",
                    )
                    .await?;
                return Ok(());
            }
            Err(_) => {
                let _ = self
                    .fail_without_first_event(
                        &session_id,
                        &turn_id,
                        &request_id,
                        None,
                        &assistant_model,
                        stream_rx,
                        "timed out waiting for first timeline event",
                    )
                    .await?;
                return Ok(());
            }
        };
        let progress_event = first_event.clone();
        self.event_service
            .record_talos_event(first_event)
            .await
            .map_err(|err| TalosError::StorageError(format!("event record failed: {err}")))?;
        self.record_completed_turn_progress(&progress_event, &assistant_model)
            .await;
        spawn_event_forwarder(
            self.event_service.clone(),
            self.executor.clone(),
            self.sessions.clone(),
            self.session_runtime.clone(),
            assistant_model,
            completed_turn_from_event(&progress_event),
            stream_rx,
        );
        Ok(())
    }

    async fn pending_dynamic_resume_context(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
    ) -> TalosResult<ActiveResumeContext> {
        let active = self
            .event_service
            .get_active_turn(session_id)
            .await
            .map_err(|err| TalosError::StorageError(format!("active turn lookup failed: {err}")))?
            .ok_or_else(|| {
                TalosError::Conflict("No pending user action for this session.".to_string())
            })?;
        if active.status != ActiveTurnStatus::WaitingUserAction {
            return Err(TalosError::Conflict(
                "The active turn is not waiting for user action.".to_string(),
            ));
        }
        let message_id = active.message_id.clone().ok_or_else(|| {
            TalosError::ValidationError(
                "Pending user action is missing its original message id.".to_string(),
            )
        })?;
        let Some(PendingUserAction::DynamicToolResult(pending)) = active.pending_user_action else {
            return Err(TalosError::Conflict(
                "The pending user action is not a dynamic tool result.".to_string(),
            ));
        };
        if pending.call_id != call_id || pending.tool_name != tool_name {
            return Err(TalosError::Conflict(format!(
                "Pending dynamic result mismatch: expected {} {}, got {} {}",
                pending.tool_name, pending.call_id, tool_name, call_id
            )));
        }
        let item_id = pending.item_id.ok_or_else(|| {
            TalosError::ValidationError(
                "Pending dynamic tool result is missing its original timeline item id.".to_string(),
            )
        })?;
        let tool_call = AgentDispatchResumeToolCall {
            item_id,
            call_id: pending.call_id,
            tool_name: pending.tool_name,
        };
        Ok(ActiveResumeContext {
            turn_id: active.turn_id,
            message_id,
            gateway_resume: Some(AgentDispatchResumeContext {
                tool_call: Some(tool_call),
            }),
        })
    }

    async fn fail_without_first_event(
        &self,
        session_id: &str,
        turn_id: &str,
        request_id: &str,
        idempotency_key: Option<&str>,
        assistant_model: &str,
        stream_rx: mpsc::Receiver<AgentEvent>,
        message: &str,
    ) -> TalosResult<MessageCommandResult> {
        let error_event = AgentEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: None,
            event_type: talos_types::talos::AgentEventType::TurnError,
            turn_id: turn_id.to_string(),
            message_id: format!("msg_{}", Uuid::new_v4().simple()),
            session_id: session_id.to_string(),
            sequence: 0,
            payload: AgentEventPayload::TurnError(TalosTurnErrorEvent {
                request_id: request_id.to_string(),
                turn_id: turn_id.to_string(),
                error: message.to_string(),
                error_info: None,
                failed_at_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
            }),
        };
        let message_id = error_event.message_id.clone();
        if let Err(err) = self.event_service.record_talos_event(error_event).await {
            error!(
                session_id = %session_id,
                turn_id = %turn_id,
                error = %err,
                "failed to persist first-event timeout error event"
            );
        }
        spawn_event_forwarder(
            self.event_service.clone(),
            self.executor.clone(),
            self.sessions.clone(),
            self.session_runtime.clone(),
            assistant_model.to_string(),
            None,
            stream_rx,
        );

        let result = MessageCommandResult {
            request_id: request_id.to_string(),
            message_id: Some(message_id.clone()),
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
            status: IdempotencyStatus::FailedNoFirstEvent,
        };
        if let Some(key) = idempotency_key {
            if let Err(err) = self
                .idempotency_store
                .complete(
                    session_id,
                    key,
                    IdempotencyStatus::FailedNoFirstEvent,
                    Some(&message_id),
                    &result,
                    chrono::Utc::now().timestamp_millis(),
                )
                .await
            {
                error!(
                    session_id = %session_id,
                    turn_id = %turn_id,
                    idempotency_key = %key,
                    error = %err,
                    "failed to persist first-event timeout idempotency state"
                );
            }
        }

        Err(TalosError::Timeout(message.to_string()))
    }

    async fn record_completed_turn_progress(&self, event: &AgentEvent, assistant_model: &str) {
        record_completed_turn_progress(
            self.sessions.as_ref(),
            self.session_runtime.as_ref(),
            event,
            assistant_model,
        )
        .await;
    }
}
