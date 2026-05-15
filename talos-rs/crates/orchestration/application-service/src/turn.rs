use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use talos_error::{TalosError, TalosResult};
use talos_gateway::{Gateway, GatewayRequest};
use talos_session_store::{
    ActiveTurnStatus, IdempotencyRecord, IdempotencyReservation, IdempotencyStatus,
    MessageCommandAck, MessageIdempotencyStore, PendingUserAction, TimelineEventService,
};
use talos_types::gateway::{GatewayResumeContext, GatewayResumeToolCall};
use talos_types::talos::TalosInputItem;
use talos_types::talos::{
    TalosContext, TalosMessageRequest, TalosTimelineEvent, TalosTimelineEventPayload,
    TalosTurnErrorEvent,
};
use talos_types::{TalosConfig, TalosSession};
use tokio::sync::mpsc;
use tracing::error;
use uuid::Uuid;

#[derive(Clone)]
pub struct TurnService {
    gateway: Arc<dyn Gateway>,
    event_service: Arc<TimelineEventService>,
    idempotency_store: Arc<dyn MessageIdempotencyStore>,
    base_config: TalosConfig,
}

#[derive(Debug, Clone)]
pub struct TurnRetryRequest {
    pub input: Vec<TalosInputItem>,
    pub workspace_id: Option<String>,
    pub client_message_id: String,
    pub retry_of_message_id: String,
}

struct ActiveResumeContext {
    turn_id: String,
    message_id: String,
    gateway_resume: Option<GatewayResumeContext>,
}

impl TurnService {
    pub fn new(
        gateway: Arc<dyn Gateway>,
        event_service: Arc<TimelineEventService>,
        idempotency_store: Arc<dyn MessageIdempotencyStore>,
        base_config: TalosConfig,
    ) -> Self {
        Self {
            gateway,
            event_service,
            idempotency_store,
            base_config,
        }
    }

    pub async fn submit_message(
        &self,
        session: TalosSession,
        request: TalosMessageRequest,
        idempotency_key: Option<String>,
    ) -> TalosResult<MessageCommandAck> {
        validate_text_input(&request.input)?;
        let session_id = session.context.session_id.clone();
        let request_hash = message_request_hash(&session_id, &request)?;
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
                    return Ok(record.ack.unwrap_or_else(|| MessageCommandAck {
                        request_id,
                        message_id: record.message_id,
                        session_id: record.session_id,
                        turn_id: record.turn_id,
                        status: record.status,
                    }));
                }
            }
        }
        self.reject_active_turn(&session_id).await?;

        let context = TalosContext {
            source_type: session.context.source_type.clone(),
            provider: session.context.provider.clone(),
            user_id: session.context.user_id.clone(),
            message_id: session.context.message_id.clone(),
            trace_id: session.context.trace_id.clone(),
            channel_id: session.context.channel_id.clone(),
            thread_id: session.context.thread_id.clone(),
            project_root: session.context.project_root.clone(),
            environment: session.context.environment.clone(),
        };
        let gateway_request = GatewayRequest {
            session_id: session_id.clone(),
            turn_id: Some(turn_id.clone()),
            workspace_id: request.workspace_id.clone(),
            resume_context: None,
            input: request.input.clone(),
            context,
        };

        let mut stream_rx = self
            .gateway
            .handle_request_stream(gateway_request)
            .await
            .map_err(|err| TalosError::GatewayError(err.to_string()))?;
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
                        stream_rx,
                        "message stream closed before first event",
                    )
                    .await;
            }
            Err(_) => {
                if let Err(err) = self.gateway.cancel_active_turn(&session_id).await {
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
                        stream_rx,
                        "timed out waiting for first timeline event",
                    )
                    .await;
            }
        };
        let message_id = first_event.message_id.clone();
        self.event_service
            .record_talos_event(first_event)
            .await
            .map_err(|err| TalosError::StorageError(format!("event record failed: {err}")))?;
        spawn_event_forwarder(self.event_service.clone(), stream_rx);

        let ack = MessageCommandAck {
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
                    &ack,
                    chrono::Utc::now().timestamp_millis(),
                )
                .await
                .map_err(|err| TalosError::IdempotencyError(err.to_string()))?;
        }

        Ok(ack)
    }

    pub async fn submit_tool_approval(
        &self,
        session: TalosSession,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()> {
        let session_id = session.context.session_id.clone();
        let resume = self
            .pending_resume_context(&session_id, call_id, tool_name)
            .await?;
        self.gateway
            .submit_tool_approval(&session_id, call_id, tool_name, approved, reason)
            .await?;
        self.event_service
            .resolve_pending_user_action(&session_id)
            .await
            .map_err(|err| TalosError::StorageError(format!("active turn update failed: {err}")))?;
        self.resume_after_user_action(session, resume).await
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
        self.gateway
            .submit_dynamic_tool_result(&session_id, call_id, tool_name, output, success)
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
        request: TurnRetryRequest,
    ) -> TalosResult<MessageCommandAck> {
        validate_retry_request(&request)?;
        validate_text_input(&request.input)?;
        let session_id = session.context.session_id.clone();
        let message_request = TalosMessageRequest {
            input: request.input.clone(),
            workspace_id: request.workspace_id.clone(),
        };
        let request_hash = message_request_hash(&session_id, &message_request)?;
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
            return Ok(ack_from_idempotency_record(record));
        }

        self.validate_retry_attempt(&session_id, &request).await?;
        self.submit_message(session, message_request, Some(idempotency_key))
            .await
    }

    async fn validate_retry_attempt(
        &self,
        session_id: &str,
        request: &TurnRetryRequest,
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
            | ActiveTurnStatus::Cancelling => Err(TalosError::Conflict(
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
        let context = TalosContext {
            source_type: session.context.source_type.clone(),
            provider: session.context.provider.clone(),
            user_id: session.context.user_id.clone(),
            message_id: resume.message_id,
            trace_id: session.context.trace_id.clone(),
            channel_id: session.context.channel_id.clone(),
            thread_id: session.context.thread_id.clone(),
            project_root: session.context.project_root.clone(),
            environment: session.context.environment.clone(),
        };
        let gateway_request = GatewayRequest {
            session_id: session_id.clone(),
            turn_id: Some(turn_id.clone()),
            workspace_id: None,
            resume_context: resume.gateway_resume,
            input: Vec::new(),
            context,
        };
        let mut stream_rx = self
            .gateway
            .handle_request_stream(gateway_request)
            .await
            .map_err(|err| TalosError::GatewayError(err.to_string()))?;
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
                        stream_rx,
                        "timed out waiting for first timeline event",
                    )
                    .await?;
                return Ok(());
            }
        };
        self.event_service
            .record_talos_event(first_event)
            .await
            .map_err(|err| TalosError::StorageError(format!("event record failed: {err}")))?;
        spawn_event_forwarder(self.event_service.clone(), stream_rx);
        Ok(())
    }

    async fn pending_resume_context(
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
        let Some(PendingUserAction::ToolApproval(pending)) = active.pending_user_action else {
            return Err(TalosError::Conflict(
                "The pending user action is not a tool approval.".to_string(),
            ));
        };
        if pending.call_id != call_id || pending.tool_name != tool_name {
            return Err(TalosError::Conflict(format!(
                "Pending approval mismatch: expected {} {}, got {} {}",
                pending.tool_name, pending.call_id, tool_name, call_id
            )));
        }
        let tool_call = pending.item_id.map(|item_id| GatewayResumeToolCall {
            item_id,
            call_id: pending.call_id,
            tool_name: pending.tool_name,
            arguments: pending.arguments,
        });
        Ok(ActiveResumeContext {
            turn_id: active.turn_id,
            message_id,
            gateway_resume: Some(GatewayResumeContext { tool_call }),
        })
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
        let tool_call = pending.item_id.map(|item_id| GatewayResumeToolCall {
            item_id,
            call_id: pending.call_id,
            tool_name: pending.tool_name,
            arguments: serde_json::Value::Null.into(),
        });
        Ok(ActiveResumeContext {
            turn_id: active.turn_id,
            message_id,
            gateway_resume: Some(GatewayResumeContext { tool_call }),
        })
    }

    async fn fail_without_first_event(
        &self,
        session_id: &str,
        turn_id: &str,
        request_id: &str,
        idempotency_key: Option<&str>,
        stream_rx: mpsc::Receiver<TalosTimelineEvent>,
        message: &str,
    ) -> TalosResult<MessageCommandAck> {
        let error_event = TalosTimelineEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: None,
            event_type: talos_types::talos::TalosTimelineEventType::TurnError,
            turn_id: turn_id.to_string(),
            message_id: format!("msg_{}", Uuid::new_v4().simple()),
            session_id: session_id.to_string(),
            sequence: 0,
            payload: TalosTimelineEventPayload::TurnError(TalosTurnErrorEvent {
                request_id: request_id.to_string(),
                turn_id: turn_id.to_string(),
                error: message.to_string(),
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
        spawn_event_forwarder(self.event_service.clone(), stream_rx);

        let ack = MessageCommandAck {
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
                    &ack,
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

fn ack_from_idempotency_record(record: IdempotencyRecord) -> MessageCommandAck {
    record.ack.unwrap_or_else(|| MessageCommandAck {
        request_id: format!("req_{}", Uuid::new_v4().simple()),
        message_id: record.message_id,
        session_id: record.session_id,
        turn_id: record.turn_id,
        status: record.status,
    })
}

fn spawn_event_forwarder(
    event_service: Arc<TimelineEventService>,
    mut stream_rx: mpsc::Receiver<TalosTimelineEvent>,
) {
    tokio::spawn(async move {
        while let Some(event) = stream_rx.recv().await {
            if let Err(err) = event_service.record_talos_event(event).await {
                error!(error = %err, "failed to persist timeline stream event");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use talos_gateway::GatewayResponse;
    use talos_session_store::{
        ActiveTurnStore, ActiveTurnUpdate, DatabaseSessionStore, PendingToolApproval,
        TimelineEventId, TimelineEventRetentionConfig, TimelineEventStore,
    };
    use talos_storage::SqliteStorage;
    use talos_types::talos::{
        StopReason, TalosProvider, TalosSourceType, TalosTurnCompletedEvent, TalosUsage,
    };
    use talos_types::{SessionConf, SessionContext, SessionState};

    #[derive(Default)]
    struct RecordingGateway {
        stream_request: Mutex<Option<GatewayRequest>>,
        approvals: Mutex<Vec<(String, String, String, bool)>>,
    }

    #[async_trait]
    impl Gateway for RecordingGateway {
        async fn handle_request(&self, request: GatewayRequest) -> TalosResult<GatewayResponse> {
            self.stream_request.lock().unwrap().replace(request);
            Ok(GatewayResponse {
                content: String::new(),
                output_items: Vec::new(),
                reasoning: Vec::new(),
                stop_reason: StopReason::EndTurn,
                usage: TalosUsage::default(),
                success: true,
                model: String::new(),
                session_id: "s1".to_string(),
            })
        }

        async fn handle_request_stream(
            &self,
            request: GatewayRequest,
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
                }),
            };
            tx.send(event).await.unwrap();
            Ok(rx)
        }

        async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
            Ok(())
        }

        async fn submit_tool_approval(
            &self,
            session_id: &str,
            call_id: &str,
            tool_name: &str,
            approved: bool,
            _reason: Option<String>,
        ) -> TalosResult<()> {
            self.approvals.lock().unwrap().push((
                session_id.to_string(),
                call_id.to_string(),
                tool_name.to_string(),
                approved,
            ));
            Ok(())
        }

        async fn submit_dynamic_tool_result(
            &self,
            _session_id: &str,
            _call_id: &str,
            _tool_name: &str,
            _output: String,
            _success: bool,
        ) -> TalosResult<()> {
            Ok(())
        }

        async fn start(&self) -> TalosResult<()> {
            Ok(())
        }
    }

    async fn test_service(
        gateway: Arc<RecordingGateway>,
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
        (
            TurnService::new(
                gateway,
                event_service.clone(),
                idempotency_store,
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

    #[tokio::test]
    async fn submit_tool_approval_resumes_original_turn_and_message() {
        let gateway = Arc::new(RecordingGateway::default());
        let (service, _event_service, store) = test_service(gateway.clone()).await;
        store
            .upsert_active_turn(ActiveTurnUpdate {
                session_id: "s1".to_string(),
                turn_id: "turn-original".to_string(),
                message_id: Some("msg-original".to_string()),
                status: ActiveTurnStatus::WaitingUserAction,
                last_event_id: Some(TimelineEventId::from_seq(9)),
                pending_user_action: Some(PendingUserAction::ToolApproval(PendingToolApproval {
                    item_id: Some("tool-original".to_string()),
                    call_id: "call-1".to_string(),
                    tool_name: "Bash".to_string(),
                    prompt: "Approve Bash?".to_string(),
                    arguments: serde_json::json!({"command":"pwd"}).into(),
                })),
                updated_at: 1,
            })
            .await
            .unwrap();

        service
            .submit_tool_approval(test_session(), "call-1", "Bash", true, None)
            .await
            .unwrap();

        let request = gateway
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
        assert_eq!(tool_call.tool_name, "Bash");

        let active = store.get_active_turn("s1").await.unwrap().unwrap();
        assert_eq!(active.status, ActiveTurnStatus::Completed);
        assert_eq!(active.pending_user_action, None);
    }
}

pub fn message_request_hash(
    session_id: &str,
    request: &TalosMessageRequest,
) -> TalosResult<String> {
    let canonical = serde_json::json!({
        "session_id": session_id,
        "workspace_id": &request.workspace_id,
        "input": &request.input,
    });
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|err| TalosError::SerializationError(err.to_string()))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}
