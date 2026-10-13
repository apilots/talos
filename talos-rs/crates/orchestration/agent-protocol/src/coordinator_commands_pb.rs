//! Protobuf projection of the original closed product command types.
use crate::{harness_pb, ActiveSessionCommand, ActiveSessionReply, ActiveSessionTarget};
use agent_pb as pb;
use talos_agent_api::{
    InterruptTurnRequest, InterruptTurnResponse, SubmitTurnInput, SubmitTurnInputRequest,
    SubmitTurnInputResponse,
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;

fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Active Session command payload is invalid.",
    )
}
fn input(value: AgentUserInput) -> pb::ProductUserInput {
    pb::ProductUserInput {
        content: value
            .content
            .into_iter()
            .map(harness_pb::encode_user_content)
            .collect(),
    }
}
fn read_input(value: Option<pb::ProductUserInput>) -> TalosResult<AgentUserInput> {
    Ok(AgentUserInput {
        content: value
            .ok_or_else(invalid)?
            .content
            .into_iter()
            .map(harness_pb::decode_user_content)
            .collect::<TalosResult<_>>()?,
    })
}

pub fn encode_request(
    target: ActiveSessionTarget,
    command: ActiveSessionCommand,
) -> TalosResult<pb::ActiveSessionCommandRequest> {
    use pb::active_session_command_request::Command as Wire;
    command.validate_target(&target)?;
    let command = match command {
        ActiveSessionCommand::Input(request) => {
            let value = match request.input {
                SubmitTurnInput::Message { input: value } => {
                    pb::coordinator_turn_input::Input::Message(input(value))
                }
                SubmitTurnInput::QueuedMessage { message_id } => {
                    pb::coordinator_turn_input::Input::QueuedMessageId(message_id)
                }
                SubmitTurnInput::UserActionResponse {
                    action_id,
                    response,
                } => {
                    let encoded =
                        harness_pb::encode_user_content(AgentUserContent::UserActionResponse {
                            action_id,
                            response,
                        });
                    let Some(pb::user_content::Content::UserActionResponse(response)) =
                        encoded.content
                    else {
                        return Err(invalid());
                    };
                    pb::coordinator_turn_input::Input::ActionResponse(response)
                }
            };
            Wire::Input(pb::CoordinatorTurnInput {
                client_request_id: request.client_request_id,
                input: Some(value),
            })
        }
        ActiveSessionCommand::Interrupt(_) => Wire::Interrupt(pb::Empty {}),
        ActiveSessionCommand::AddMessage(request) => Wire::AddMessage(input(request.input)),
        ActiveSessionCommand::ListMessages(_) => Wire::ListMessages(pb::Empty {}),
        ActiveSessionCommand::UpdateMessage(request) => {
            Wire::UpdateMessage(pb::CoordinatorQueueMessage {
                message_id: request.message_id,
                input: Some(input(request.input)),
            })
        }
        ActiveSessionCommand::DeleteMessage(request) => Wire::DeleteMessage(request.message_id),
        ActiveSessionCommand::ReorderMessages(request) => {
            Wire::ReorderMessages(pb::CoordinatorQueueOrder {
                message_ids: request.message_ids,
            })
        }
        ActiveSessionCommand::StartMessage(request) => {
            Wire::StartMessage(pb::CoordinatorQueueStart {
                message_id: request.message_id,
            })
        }
    };
    let request = pb::ActiveSessionCommandRequest {
        auth: None,
        computer_id: target.computer_id,
        source_peer_id: target.coordinator_id,
        identity: Some(harness_pb::encode_turn_ref(target.identity)),
        command: Some(command),
    };
    Ok(request)
}

pub fn decode_request(
    request: pb::ActiveSessionCommandRequest,
) -> TalosResult<(ActiveSessionTarget, ActiveSessionCommand)> {
    if uuid::Uuid::parse_str(&request.computer_id).is_err()
        || uuid::Uuid::parse_str(&request.source_peer_id).is_err()
    {
        return Err(invalid());
    }
    let identity = harness_pb::decode_turn_ref(request.identity)?;
    let id = identity.session_id.clone();
    use pb::active_session_command_request::Command as Wire;
    let command = match request.command.ok_or_else(invalid)? {
        Wire::Input(request) => {
            let input = match request.input.ok_or_else(invalid)? {
                pb::coordinator_turn_input::Input::Message(value) => SubmitTurnInput::Message {
                    input: read_input(Some(value))?,
                },
                pb::coordinator_turn_input::Input::QueuedMessageId(message_id) => {
                    SubmitTurnInput::QueuedMessage { message_id }
                }
                pb::coordinator_turn_input::Input::ActionResponse(value) => {
                    let content = harness_pb::decode_user_content(pb::UserContent {
                        content: Some(pb::user_content::Content::UserActionResponse(value)),
                    })?;
                    let AgentUserContent::UserActionResponse {
                        action_id,
                        response,
                    } = content
                    else {
                        return Err(invalid());
                    };
                    SubmitTurnInput::UserActionResponse {
                        action_id,
                        response,
                    }
                }
            };
            ActiveSessionCommand::Input(SubmitTurnInputRequest {
                identity: identity.clone(),
                input,
                client_request_id: request.client_request_id,
            })
        }
        Wire::Interrupt(_) => ActiveSessionCommand::Interrupt(InterruptTurnRequest {
            identity: identity.clone(),
        }),
        Wire::AddMessage(value) => ActiveSessionCommand::AddMessage(MessageQueueAddRequest {
            session_id: id,
            input: read_input(Some(value))?,
        }),
        Wire::ListMessages(_) => {
            ActiveSessionCommand::ListMessages(MessageQueueListRequest { session_id: id })
        }
        Wire::UpdateMessage(value) => {
            ActiveSessionCommand::UpdateMessage(MessageQueueUpdateRequest {
                session_id: id,
                message_id: value.message_id,
                input: read_input(value.input)?,
            })
        }
        Wire::DeleteMessage(message_id) => {
            ActiveSessionCommand::DeleteMessage(MessageQueueDeleteRequest {
                session_id: id,
                message_id,
            })
        }
        Wire::ReorderMessages(value) => {
            ActiveSessionCommand::ReorderMessages(MessageQueueReorderRequest {
                session_id: id,
                message_ids: value.message_ids,
            })
        }
        Wire::StartMessage(value) => ActiveSessionCommand::StartMessage(MessageQueueStartRequest {
            session_id: id,
            message_id: value.message_id,
        }),
    };
    Ok((
        ActiveSessionTarget {
            computer_id: request.computer_id,
            coordinator_id: request.source_peer_id,
            identity,
        },
        command,
    ))
}

pub fn encode_reply(reply: ActiveSessionReply) -> pb::ActiveSessionCommandResponse {
    use pb::active_session_command_response::Result as Wire;
    let result = match reply {
        ActiveSessionReply::Input(value) => Wire::Input(pb::CoordinatorInputResult {
            request_id: value.request_id,
            identity: Some(harness_pb::encode_turn_ref(value.identity)),
            message_id: value.message_id,
        }),
        ActiveSessionReply::Interrupt(value) => Wire::Interrupted(pb::CoordinatorInterruptResult {
            identity: Some(harness_pb::encode_turn_ref(value.identity)),
            outcome: Some(harness_pb::encode_interrupt_outcome(value.outcome)),
        }),
        ActiveSessionReply::Queue(value) => Wire::Queue(pb::CoordinatorMessageQueue {
            revision: value.revision,
            max_messages: value.max_messages,
            messages: value
                .messages
                .into_iter()
                .map(|message| pb::CoordinatorQueueMessage {
                    message_id: message.id,
                    input: Some(input(message.input)),
                })
                .collect(),
        }),
        ActiveSessionReply::Started(value) => Wire::Started(pb::CoordinatorStartedTurn {
            request_id: value.request_id,
            message_id: value.message_id,
            session_id: value.session_id,
            turn_id: value.turn_id,
            status: match value.status {
                TurnCommandStatus::Accepted => pb::ProductTurnCommandStatus::Accepted as i32,
                TurnCommandStatus::Pending => pb::ProductTurnCommandStatus::Pending as i32,
                TurnCommandStatus::Failed => pb::ProductTurnCommandStatus::Failed as i32,
                TurnCommandStatus::FailedNoFirstEvent => {
                    pb::ProductTurnCommandStatus::FailedNoFirstEvent as i32
                }
            },
        }),
    };
    pb::ActiveSessionCommandResponse {
        result: Some(result),
    }
}

pub fn decode_reply(reply: pb::ActiveSessionCommandResponse) -> TalosResult<ActiveSessionReply> {
    use pb::active_session_command_response::Result as Wire;
    Ok(match reply.result.ok_or_else(invalid)? {
        Wire::Input(value) => ActiveSessionReply::Input(SubmitTurnInputResponse {
            request_id: value.request_id,
            identity: harness_pb::decode_turn_ref(value.identity)?,
            message_id: value.message_id,
        }),
        Wire::Interrupted(value) => ActiveSessionReply::Interrupt(InterruptTurnResponse {
            identity: harness_pb::decode_turn_ref(value.identity)?,
            outcome: harness_pb::decode_interrupt_outcome(value.outcome.ok_or_else(invalid)?)?,
        }),
        Wire::Queue(value) => ActiveSessionReply::Queue(MessageQueue {
            revision: value.revision,
            max_messages: value.max_messages,
            messages: value
                .messages
                .into_iter()
                .map(|message| {
                    Ok(QueuedMessage {
                        id: message.message_id,
                        input: read_input(message.input)?,
                    })
                })
                .collect::<TalosResult<_>>()?,
        }),
        Wire::Started(value) => ActiveSessionReply::Started(StartTurnResponse {
            request_id: value.request_id,
            message_id: value.message_id,
            session_id: value.session_id,
            turn_id: value.turn_id,
            status: match pb::ProductTurnCommandStatus::try_from(value.status) {
                Ok(pb::ProductTurnCommandStatus::Accepted) => TurnCommandStatus::Accepted,
                Ok(pb::ProductTurnCommandStatus::Pending) => TurnCommandStatus::Pending,
                Ok(pb::ProductTurnCommandStatus::Failed) => TurnCommandStatus::Failed,
                Ok(pb::ProductTurnCommandStatus::FailedNoFirstEvent) => {
                    TurnCommandStatus::FailedNoFirstEvent
                }
                _ => return Err(invalid()),
            },
        }),
        Wire::Failure(error) => return Err(crate::error_pb::decode_failure(error)?),
    })
}
