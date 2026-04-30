use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use talos_error::{TalosError, TalosResult};
use talos_gateway::{Gateway, GatewayRequest};
use talos_session_store::{
    ActiveTurnStatus, IdempotencyRecord, IdempotencyReservation, IdempotencyStatus,
    MessageCommandAck, MessageIdempotencyStore, SessionEventService,
};
use talos_types::talos::TalosInputItem;
use talos_types::talos::{
    TalosContext, TalosEvent, TalosEventPayload, TalosEventType, TalosMessageErrorEvent,
    TalosMessageRequest,
};
use talos_types::{TalosConfig, TalosSession};
use tokio::sync::mpsc;
use tracing::error;
use uuid::Uuid;

#[derive(Clone)]
pub struct TurnService {
    gateway: Arc<dyn Gateway>,
    event_service: Arc<SessionEventService>,
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

impl TurnService {
    pub fn new(
        gateway: Arc<dyn Gateway>,
        event_service: Arc<SessionEventService>,
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
                    .session_events
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
            workspace_id: request.workspace_id.clone(),
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
                .session_events
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
                        "timed out waiting for first message event",
                    )
                    .await;
            }
        };
        let message_id = first_event.message_id.clone();
        self.event_service
            .record_talos_event(Some(turn_id.clone()), first_event)
            .await
            .map_err(|err| TalosError::StorageError(format!("event record failed: {err}")))?;
        spawn_event_forwarder(self.event_service.clone(), Some(turn_id.clone()), stream_rx);

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

    async fn fail_without_first_event(
        &self,
        session_id: &str,
        turn_id: &str,
        request_id: &str,
        idempotency_key: Option<&str>,
        stream_rx: mpsc::Receiver<TalosEvent>,
        message: &str,
    ) -> TalosResult<MessageCommandAck> {
        let error_event = TalosEvent {
            event_type: TalosEventType::MessageError,
            message_id: format!("msg_{}", Uuid::new_v4().simple()),
            session_id: session_id.to_string(),
            sequence: 0,
            payload: TalosEventPayload::MessageError(TalosMessageErrorEvent {
                request_id: request_id.to_string(),
                error: message.to_string(),
            }),
        };
        let message_id = error_event.message_id.clone();
        if let Err(err) = self
            .event_service
            .record_talos_event(Some(turn_id.to_string()), error_event)
            .await
        {
            error!(
                session_id = %session_id,
                turn_id = %turn_id,
                error = %err,
                "failed to persist first-event timeout error event"
            );
        }
        spawn_event_forwarder(
            self.event_service.clone(),
            Some(turn_id.to_string()),
            stream_rx,
        );

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
    event_service: Arc<SessionEventService>,
    turn_id: Option<String>,
    mut stream_rx: mpsc::Receiver<TalosEvent>,
) {
    tokio::spawn(async move {
        while let Some(event) = stream_rx.recv().await {
            if let Err(err) = event_service
                .record_talos_event(turn_id.clone(), event)
                .await
            {
                error!(error = %err, "failed to persist session stream event");
            }
        }
    });
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
