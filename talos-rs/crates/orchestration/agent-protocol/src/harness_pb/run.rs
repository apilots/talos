use agent_pb as pb;
use talos_agent_api::AgentInterruptOutcome;
use talos_config::Config;
use talos_error::TalosResult;

use super::context::{
    decode_context, decode_metrics, decode_turn_ref, encode_context, encode_metrics,
    encode_turn_ref,
};
use super::event::{decode_event, encode_event};
use super::invalid;
use super::message::{decode_message, encode_message};
use crate::error_pb;
use crate::{
    AgentInputParams, AgentInterruptParams, AgentRunIntent, AgentRunParams, AgentRunSettlement,
};

pub fn encode_run_params(value: AgentRunParams) -> TalosResult<pb::RunParams> {
    value.validate()?;
    Ok(pb::RunParams {
        subagent_limit: value.subagent_limit.map(std::num::NonZeroU32::get),
        command: value.command.map(encode_command),
        automation_origin: value
            .automation_origin
            .map(crate::automation_pb::encode_origin),
        context: Some(encode_context(value.context)?),
        input: Some(encode_message(value.input)?),
        intent: match value.intent {
            AgentRunIntent::Start => pb::AgentRunIntent::Start,
            AgentRunIntent::RespondUserAction => pb::AgentRunIntent::RespondUserAction,
        } as i32,
    })
}

pub fn decode_run_params(value: pb::RunParams, config: &Config) -> TalosResult<AgentRunParams> {
    let value = AgentRunParams {
        subagent_limit: value
            .subagent_limit
            .map(|value| std::num::NonZeroU32::new(value).ok_or_else(invalid))
            .transpose()?,
        command: value.command.map(decode_command).transpose()?,
        automation_origin: value
            .automation_origin
            .map(crate::automation_pb::decode_origin)
            .transpose()?,
        context: decode_context(value.context, config)?,
        input: decode_message(value.input.ok_or_else(invalid)?)?,
        intent: match pb::AgentRunIntent::try_from(value.intent).map_err(|_| invalid())? {
            pb::AgentRunIntent::Start => AgentRunIntent::Start,
            pb::AgentRunIntent::RespondUserAction => AgentRunIntent::RespondUserAction,
            pb::AgentRunIntent::Unspecified => return Err(invalid()),
        },
    };
    value.validate()?;
    Ok(value)
}

pub fn encode_input_params(value: AgentInputParams) -> TalosResult<pb::InputParams> {
    Ok(pb::InputParams {
        command: value.command.map(encode_command),
        agent_id: value.agent_id,
        identity: Some(encode_turn_ref(value.identity)),
        input: Some(encode_message(value.input)?),
    })
}

pub fn decode_input_params(value: pb::InputParams) -> TalosResult<AgentInputParams> {
    if value.agent_id.is_empty() {
        return Err(invalid());
    }
    Ok(AgentInputParams {
        command: value.command.map(decode_command).transpose()?,
        agent_id: value.agent_id,
        identity: decode_turn_ref(value.identity)?,
        input: decode_message(value.input.ok_or_else(invalid)?)?,
    })
}

pub fn encode_interrupt_params(value: AgentInterruptParams) -> pb::InterruptParams {
    pb::InterruptParams {
        agent_id: value.agent_id,
        identity: Some(encode_turn_ref(value.identity)),
    }
}

pub fn decode_interrupt_params(value: pb::InterruptParams) -> TalosResult<AgentInterruptParams> {
    if value.agent_id.is_empty() {
        return Err(invalid());
    }
    Ok(AgentInterruptParams {
        agent_id: value.agent_id,
        identity: decode_turn_ref(value.identity)?,
    })
}

pub fn encode_admission_events(
    events: Vec<talos_agent_api::AgentEvent>,
) -> TalosResult<pb::Admission> {
    Ok(pb::Admission {
        events: events
            .into_iter()
            .map(encode_event)
            .collect::<TalosResult<_>>()?,
    })
}

pub fn decode_admission_events(
    value: pb::Admission,
) -> TalosResult<Vec<talos_agent_api::AgentEvent>> {
    value.events.into_iter().map(decode_event).collect()
}

pub fn encode_settlement(value: AgentRunSettlement) -> TalosResult<pb::Settlement> {
    Ok(pb::Settlement {
        result: Some(match value {
            AgentRunSettlement::WaitingForInput { event, metrics } => {
                pb::settlement::Result::Waiting(pb::WaitingSettlement {
                    event: Some(encode_event(event)?),
                    metrics: Some(encode_metrics(metrics)),
                })
            }
            AgentRunSettlement::Finished {
                event,
                metrics,
                result,
            } => pb::settlement::Result::Finished(pb::FinishedSettlement {
                event: Some(encode_event(event)?),
                metrics: Some(encode_metrics(metrics)),
                result: Some(match result {
                    Ok(()) => pb::finished_settlement::Result::Success(pb::Empty {}),
                    Err(error) => {
                        pb::finished_settlement::Result::Failure(error_pb::encode_failure(&error))
                    }
                }),
            }),
        }),
    })
}

pub fn decode_settlement(value: pb::Settlement) -> TalosResult<AgentRunSettlement> {
    match value.result.ok_or_else(invalid)? {
        pb::settlement::Result::Waiting(value) => Ok(AgentRunSettlement::WaitingForInput {
            event: decode_event(value.event.ok_or_else(invalid)?)?,
            metrics: decode_metrics(value.metrics)?,
        }),
        pb::settlement::Result::Finished(value) => {
            let result = match value.result.ok_or_else(invalid)? {
                pb::finished_settlement::Result::Success(_) => Ok(()),
                pb::finished_settlement::Result::Failure(value) => {
                    Err(error_pb::decode_failure(value)?)
                }
            };
            Ok(AgentRunSettlement::Finished {
                event: decode_event(value.event.ok_or_else(invalid)?)?,
                metrics: decode_metrics(value.metrics)?,
                result,
            })
        }
    }
}

pub fn encode_interrupt_outcome(value: AgentInterruptOutcome) -> pb::InterruptResult {
    pb::InterruptResult {
        outcome: Some(match value {
            AgentInterruptOutcome::Cancelled => {
                pb::interrupt_result::Outcome::Cancelled(pb::Empty {})
            }
            AgentInterruptOutcome::Completed => {
                pb::interrupt_result::Outcome::Completed(pb::Empty {})
            }
            AgentInterruptOutcome::CleanupUnconfirmed { task_count } => {
                pb::interrupt_result::Outcome::UnconfirmedTaskCount(task_count)
            }
        }),
    }
}

pub fn decode_interrupt_outcome(value: pb::InterruptResult) -> TalosResult<AgentInterruptOutcome> {
    match value.outcome.ok_or_else(invalid)? {
        pb::interrupt_result::Outcome::Cancelled(_) => Ok(AgentInterruptOutcome::Cancelled),
        pb::interrupt_result::Outcome::Completed(_) => Ok(AgentInterruptOutcome::Completed),
        pb::interrupt_result::Outcome::UnconfirmedTaskCount(task_count) => {
            Ok(AgentInterruptOutcome::CleanupUnconfirmed { task_count })
        }
    }
}

pub fn encode_command(value: crate::AgentCommandIdentity) -> pb::AgentCommandIdentity {
    pb::AgentCommandIdentity {
        command: match value.command {
            talos_types::IdempotentCommand::TurnSubmission => {
                pb::AgentCommandKind::TurnSubmission as i32
            }
            talos_types::IdempotentCommand::TurnInput => pb::AgentCommandKind::TurnInput as i32,
        },
        idempotency_key: value.idempotency_key,
        request_id: value.request_id,
    }
}
pub fn decode_command(value: pb::AgentCommandIdentity) -> TalosResult<crate::AgentCommandIdentity> {
    if value.idempotency_key.is_empty() || value.request_id.is_empty() {
        return Err(invalid());
    }
    Ok(crate::AgentCommandIdentity {
        command: match pb::AgentCommandKind::try_from(value.command) {
            Ok(pb::AgentCommandKind::TurnSubmission) => {
                talos_types::IdempotentCommand::TurnSubmission
            }
            Ok(pb::AgentCommandKind::TurnInput) => talos_types::IdempotentCommand::TurnInput,
            _ => return Err(invalid()),
        },
        idempotency_key: value.idempotency_key,
        request_id: value.request_id,
    })
}
