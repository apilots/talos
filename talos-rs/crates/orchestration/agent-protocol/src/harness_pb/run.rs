use agent_pb as pb;
use talos_agent_api::{AgentContinuationRef, AgentInterruptOutcome};
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
    AgentInputParams, AgentInterruptParams, AgentRunCheckpoint, AgentRunParams, AgentRunSettlement,
};

pub fn encode_run_params(value: AgentRunParams) -> TalosResult<pb::RunParams> {
    value.validate()?;
    Ok(pb::RunParams {
        context: Some(encode_context(value.context)?),
        input: Some(encode_message(value.input)?),
        checkpoint: value.checkpoint.map(encode_checkpoint).transpose()?,
        continuation: value.continuation.map(|value| pb::ContinuationRef {
            opaque_id: value.opaque_id,
        }),
    })
}

pub fn decode_run_params(value: pb::RunParams, config: &Config) -> TalosResult<AgentRunParams> {
    let value = AgentRunParams {
        context: decode_context(value.context, config)?,
        input: decode_message(value.input.ok_or_else(invalid)?)?,
        checkpoint: value.checkpoint.map(decode_checkpoint).transpose()?,
        continuation: value.continuation.map(|value| AgentContinuationRef {
            opaque_id: value.opaque_id,
        }),
    };
    value.validate()?;
    Ok(value)
}

pub fn encode_input_params(value: AgentInputParams) -> TalosResult<pb::InputParams> {
    Ok(pb::InputParams {
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
            AgentRunSettlement::WaitingForInput {
                event,
                continuation,
                metrics,
            } => pb::settlement::Result::Waiting(pb::WaitingSettlement {
                event: Some(encode_event(event)?),
                continuation: Some(pb::ContinuationRef {
                    opaque_id: continuation.opaque_id,
                }),
                metrics: Some(encode_metrics(metrics)),
            }),
            AgentRunSettlement::Finished {
                event,
                messages,
                metrics,
                result,
            } => pb::settlement::Result::Finished(pb::FinishedSettlement {
                event: Some(encode_event(event)?),
                messages: messages
                    .into_iter()
                    .map(encode_message)
                    .collect::<TalosResult<_>>()?,
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
            continuation: AgentContinuationRef {
                opaque_id: value.continuation.ok_or_else(invalid)?.opaque_id,
            },
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
                messages: value
                    .messages
                    .into_iter()
                    .map(decode_message)
                    .collect::<TalosResult<_>>()?,
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

fn encode_checkpoint(value: AgentRunCheckpoint) -> TalosResult<pb::Checkpoint> {
    Ok(pb::Checkpoint {
        last_sequence: value.last_sequence,
        messages: value
            .messages
            .into_iter()
            .map(encode_message)
            .collect::<TalosResult<_>>()?,
        metrics: Some(encode_metrics(value.metrics)),
    })
}

fn decode_checkpoint(value: pb::Checkpoint) -> TalosResult<AgentRunCheckpoint> {
    Ok(AgentRunCheckpoint {
        last_sequence: value.last_sequence,
        messages: value
            .messages
            .into_iter()
            .map(decode_message)
            .collect::<TalosResult<_>>()?,
        metrics: decode_metrics(value.metrics)?,
    })
}
