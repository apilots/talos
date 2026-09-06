use sha2::{Digest, Sha256};
use talos_agent::AgentInputParams;
use talos_agent_api::{
    ActiveTurnState, AgentTurnRef, SubmitTurnInput, SubmitTurnInputRequest, SubmitTurnInputResponse,
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::session::{
    CommandLease, CommandReservation, IdempotencyRecord, IdempotencyReservation, IdempotentCommand,
};
use talos_types::TurnCommandStatus;

use super::{TurnService, TURN_SUBMISSION_LEASE_MS};

enum TurnInputReservation {
    Ready {
        request_id: String,
        message_id: String,
        command: Option<CommandLease>,
    },
    Completed(SubmitTurnInputResponse),
}

impl TurnService {
    /// Deliver one typed input through the single running/waiting Turn boundary.
    pub(crate) async fn submit_turn_input(
        &self,
        session: talos_store::session::SessionRecord,
        request: SubmitTurnInputRequest,
    ) -> TalosResult<SubmitTurnInputResponse> {
        let service = self.clone();
        super::await_admission(
            async move { service.submit_turn_input_owned(session, request).await },
        )
        .await
    }

    async fn submit_turn_input_owned(
        &self,
        session: talos_store::session::SessionRecord,
        request: SubmitTurnInputRequest,
    ) -> TalosResult<SubmitTurnInputResponse> {
        if session.id != request.identity.session_id {
            return Err(TalosError::new(
                ErrorCode::ResourceNotFound,
                format!("Session {} was not found.", request.identity.session_id),
            ));
        }
        validate_client_request_id(request.client_request_id.as_deref())?;

        match request.input.clone() {
            SubmitTurnInput::Message { input } => {
                super::super::validate_active_turn_input(&input)?;
                let reservation = self
                    .reserve_turn_input_command(
                        &request,
                        IdempotentCommand::TurnInput,
                        format!("msg_input_{}", uuid::Uuid::new_v4()),
                    )
                    .await?;
                let (request_id, message_id, command) = match reservation {
                    TurnInputReservation::Ready {
                        request_id,
                        message_id,
                        command,
                    } => (request_id, message_id, command),
                    TurnInputReservation::Completed(response) => return Ok(response),
                };
                let result = async {
                    self.require_running_turn(&request.identity).await?;
                    let message = crate::turn::completed_agent_user_input(
                        &request.identity.turn_id,
                        &message_id,
                        input,
                        chrono::Utc::now().timestamp(),
                    )?;
                    self.execution
                        .admit_running_input(
                            AgentInputParams {
                                agent_id: session.agent_id.clone(),
                                identity: request.identity.clone(),
                                input: message,
                            },
                            command.clone(),
                        )
                        .await?;
                    Ok(())
                }
                .await;
                if let Err(error) = result {
                    self.release_command(command).await;
                    return Err(error);
                }
                Ok(SubmitTurnInputResponse {
                    request_id,
                    identity: request.identity,
                    message_id,
                })
            }
            SubmitTurnInput::QueuedMessage { message_id } => {
                let reservation = self
                    .reserve_turn_input_command(
                        &request,
                        IdempotentCommand::TurnInput,
                        message_id.clone(),
                    )
                    .await?;
                let (request_id, message_id, command) = match reservation {
                    TurnInputReservation::Ready {
                        request_id,
                        message_id,
                        command,
                    } => (request_id, message_id, command),
                    TurnInputReservation::Completed(response) => return Ok(response),
                };
                if let Err(error) = self
                    .steer_queued_message(
                        session,
                        &message_id,
                        &request.identity.turn_id,
                        command.clone(),
                    )
                    .await
                {
                    self.release_command(command).await;
                    return Err(error);
                }
                Ok(SubmitTurnInputResponse {
                    request_id,
                    identity: request.identity,
                    message_id,
                })
            }
            SubmitTurnInput::UserActionResponse {
                action_id,
                response,
            } => {
                let reservation = self
                    .reserve_turn_input_command(
                        &request,
                        IdempotentCommand::TurnInput,
                        format!("msg_action_{action_id}"),
                    )
                    .await?;
                let (request_id, message_id, command) = match reservation {
                    TurnInputReservation::Ready {
                        request_id,
                        message_id,
                        command,
                    } => (request_id, message_id, command),
                    TurnInputReservation::Completed(response) => return Ok(response),
                };
                let result = self
                    .submit_user_action(
                        session,
                        &request.identity.turn_id,
                        &action_id,
                        response,
                        command.clone(),
                    )
                    .await;
                if let Err(error) = result {
                    self.release_command(command).await;
                    return Err(error);
                }
                Ok(SubmitTurnInputResponse {
                    request_id,
                    identity: request.identity,
                    message_id,
                })
            }
        }
    }

    async fn reserve_turn_input_command(
        &self,
        request: &SubmitTurnInputRequest,
        command: IdempotentCommand,
        proposed_message_id: String,
    ) -> TalosResult<TurnInputReservation> {
        let Some(idempotency_key) = request.client_request_id.as_ref() else {
            return Ok(TurnInputReservation::Ready {
                request_id: response_request_id(None),
                message_id: proposed_message_id,
                command: None,
            });
        };
        let now = chrono::Utc::now().timestamp_millis();
        let reservation = self
            .execution
            .execution_store
            .reserve_command(CommandReservation {
                session_id: request.identity.session_id.clone(),
                command,
                idempotency_key: idempotency_key.clone(),
                request_hash: turn_input_hash(request)?,
                request_id: idempotency_key.clone(),
                turn_id: request.identity.turn_id.clone(),
                message_id: proposed_message_id,
                lease_id: format!("lease_{}", uuid::Uuid::new_v4().simple()),
                now,
                lease_expires_at: now.saturating_add(TURN_SUBMISSION_LEASE_MS),
                record_expires_at: Some(now.saturating_add(
                    (self.base_config.agent.command_idempotency_ttl_hours as i64) * 60 * 60 * 1000,
                )),
            })
            .await?;
        match reservation {
            IdempotencyReservation::Reserved(lease) => Ok(TurnInputReservation::Ready {
                request_id: lease.request_id.clone(),
                message_id: lease.message_id.clone(),
                command: Some(lease),
            }),
            IdempotencyReservation::Completed(record) => Ok(TurnInputReservation::Completed(
                turn_input_response(record)?,
            )),
            IdempotencyReservation::InProgress => Err(TalosError::new(
                ErrorCode::Conflict,
                "The idempotent Turn input is still in progress",
            )),
        }
    }

    async fn require_running_turn(&self, identity: &AgentTurnRef) -> TalosResult<()> {
        let active = self.canonical_active_turn(&identity.session_id).await?;
        if matches!(
            active,
            Some(ref active)
                if active.identity == *identity && active.state == ActiveTurnState::Running
        ) {
            return Ok(());
        }
        Err(TalosError::new(
            ErrorCode::ResourceNotFound,
            format!(
                "running turn {} for session {}",
                identity.turn_id, identity.session_id
            ),
        ))
    }
}

fn validate_client_request_id(client_request_id: Option<&str>) -> TalosResult<()> {
    if client_request_id.is_some_and(|request_id| request_id.trim().is_empty()) {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "clientRequestId must not be empty",
        ));
    }
    Ok(())
}

fn response_request_id(client_request_id: Option<String>) -> String {
    client_request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn turn_input_hash(request: &SubmitTurnInputRequest) -> TalosResult<String> {
    let bytes = serde_json::to_vec(request).map_err(|error| {
        TalosError::internal_with_source(
            "Failed to serialize the turn request; technical details were withheld.",
            error,
        )
    })?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn turn_input_response(record: IdempotencyRecord) -> TalosResult<SubmitTurnInputResponse> {
    if record.outcome != Some(TurnCommandStatus::Accepted) {
        return Err(TalosError::internal(
            "completed Turn input idempotency record is not accepted",
        ));
    }
    let message_id = record.message_id.ok_or_else(|| {
        TalosError::internal("completed Turn input idempotency record is missing its message id")
    })?;
    Ok(SubmitTurnInputResponse {
        request_id: record.request_id,
        identity: AgentTurnRef::new(record.session_id, record.turn_id),
        message_id,
    })
}
