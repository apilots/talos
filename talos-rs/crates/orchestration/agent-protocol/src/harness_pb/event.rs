use agent_pb as pb;
use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentMessageBlockCompleted, AgentMessageBlockDelta,
    AgentMessageBlockDeltaContent, AgentMessageBlockStarted, AgentMessageStarted, AgentStopReason,
    AgentToolCallArgumentsDelta, AgentTurnCompleted, AgentTurnFailed, AgentTurnStarted,
};
use talos_error::TalosResult;

use super::action::{decode_pending_input, encode_pending_input};
use super::compaction::{decode_compaction_delta, encode_compaction_delta};
use super::context::{decode_metrics, decode_turn_ref, encode_metrics, encode_turn_ref};
use super::invalid;
use super::message::{
    decode_block, decode_message, decode_phase, decode_tool_progress, encode_block, encode_message,
    encode_phase, encode_tool_progress,
};
use crate::error_pb;

pub fn encode_event(value: AgentEvent) -> TalosResult<pb::AgentEvent> {
    use pb::agent_event::Payload;
    let payload = match value.payload {
        AgentEventPayload::TurnStarted(_) => Payload::TurnStarted(pb::Empty {}),
        AgentEventPayload::MessageStarted(value) => {
            Payload::MessageStarted(encode_message(value.message)?)
        }
        AgentEventPayload::MessageBlockStarted(value) => {
            Payload::MessageBlockStarted(pb::BlockSnapshot {
                message_id: value.message_id,
                block: Some(encode_block(value.block)?),
            })
        }
        AgentEventPayload::MessageBlockDelta(value) => Payload::MessageBlockDelta(pb::BlockDelta {
            message_id: value.message_id,
            block_id: value.block_id,
            delta: Some(match value.delta {
                AgentMessageBlockDeltaContent::Text(text) => pb::block_delta::Delta::Text(text),
                AgentMessageBlockDeltaContent::AssistantTextPhase(phase) => {
                    pb::block_delta::Delta::AssistantTextPhase(encode_phase(phase))
                }
                AgentMessageBlockDeltaContent::ToolCallArguments(value) => {
                    pb::block_delta::Delta::ToolCallArgumentsFragment(value.json_fragment)
                }
                AgentMessageBlockDeltaContent::ToolProgress(value) => {
                    pb::block_delta::Delta::ToolProgress(encode_tool_progress(value))
                }
                AgentMessageBlockDeltaContent::ContextCompaction(value) => {
                    pb::block_delta::Delta::ContextCompaction(encode_compaction_delta(value))
                }
            }),
        }),
        AgentEventPayload::MessageBlockCompleted(value) => {
            Payload::MessageBlockCompleted(pb::BlockSnapshot {
                message_id: value.message_id,
                block: Some(encode_block(value.block)?),
            })
        }
        AgentEventPayload::MessageCompleted(value) => {
            Payload::MessageCompleted(encode_message(value)?)
        }
        AgentEventPayload::WaitingForInput(value) => {
            Payload::WaitingForInput(encode_pending_input(value))
        }
        AgentEventPayload::MetricsUpdated(value) => Payload::MetricsUpdated(encode_metrics(value)),
        AgentEventPayload::TurnCompleted(value) => {
            Payload::TurnCompleted(encode_stop_reason(value.stop_reason))
        }
        AgentEventPayload::TurnFailed(value) => Payload::TurnFailed(pb::TurnFailed {
            error: Some(error_pb::encode_api_error(value.error)),
            retryable: value.retryable,
        }),
    };
    Ok(pb::AgentEvent {
        identity: Some(encode_turn_ref(value.identity)),
        sequence: value.sequence,
        occurred_at: value.occurred_at,
        payload: Some(payload),
    })
}

pub fn decode_event(value: pb::AgentEvent) -> TalosResult<AgentEvent> {
    use pb::agent_event::Payload;
    let payload = match value.payload.ok_or_else(invalid)? {
        Payload::TurnStarted(_) => AgentEventPayload::TurnStarted(AgentTurnStarted {}),
        Payload::MessageStarted(value) => AgentEventPayload::MessageStarted(AgentMessageStarted {
            message: decode_message(value)?,
        }),
        Payload::MessageBlockStarted(value) => {
            AgentEventPayload::MessageBlockStarted(AgentMessageBlockStarted {
                message_id: value.message_id,
                block: decode_block(value.block.ok_or_else(invalid)?)?,
            })
        }
        Payload::MessageBlockDelta(value) => {
            AgentEventPayload::MessageBlockDelta(AgentMessageBlockDelta {
                message_id: value.message_id,
                block_id: value.block_id,
                delta: match value.delta.ok_or_else(invalid)? {
                    pb::block_delta::Delta::Text(text) => AgentMessageBlockDeltaContent::Text(text),
                    pb::block_delta::Delta::AssistantTextPhase(phase) => {
                        AgentMessageBlockDeltaContent::AssistantTextPhase(decode_phase(phase)?)
                    }
                    pb::block_delta::Delta::ToolCallArgumentsFragment(json_fragment) => {
                        AgentMessageBlockDeltaContent::ToolCallArguments(
                            AgentToolCallArgumentsDelta { json_fragment },
                        )
                    }
                    pb::block_delta::Delta::ToolProgress(value) => {
                        AgentMessageBlockDeltaContent::ToolProgress(decode_tool_progress(value)?)
                    }
                    pb::block_delta::Delta::ContextCompaction(value) => {
                        AgentMessageBlockDeltaContent::ContextCompaction(decode_compaction_delta(
                            value,
                        )?)
                    }
                },
            })
        }
        Payload::MessageBlockCompleted(value) => {
            AgentEventPayload::MessageBlockCompleted(AgentMessageBlockCompleted {
                message_id: value.message_id,
                block: decode_block(value.block.ok_or_else(invalid)?)?,
            })
        }
        Payload::MessageCompleted(value) => {
            AgentEventPayload::MessageCompleted(decode_message(value)?)
        }
        Payload::WaitingForInput(value) => {
            AgentEventPayload::WaitingForInput(decode_pending_input(value)?)
        }
        Payload::MetricsUpdated(value) => {
            AgentEventPayload::MetricsUpdated(decode_metrics(Some(value))?)
        }
        Payload::TurnCompleted(value) => AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: decode_stop_reason(value)?,
        }),
        Payload::TurnFailed(value) => AgentEventPayload::TurnFailed(AgentTurnFailed {
            error: error_pb::decode_api_error(value.error.ok_or_else(invalid)?)?,
            retryable: value.retryable,
        }),
    };
    Ok(AgentEvent {
        identity: decode_turn_ref(value.identity)?,
        sequence: value.sequence,
        occurred_at: value.occurred_at,
        payload,
    })
}

fn encode_stop_reason(value: AgentStopReason) -> i32 {
    match value {
        AgentStopReason::EndTurn => pb::StopReason::EndTurn as i32,
        AgentStopReason::OutputLimit => pb::StopReason::OutputLimit as i32,
        AgentStopReason::TransferUnresolved => pb::StopReason::TransferUnresolved as i32,
    }
}

fn decode_stop_reason(value: i32) -> TalosResult<AgentStopReason> {
    match pb::StopReason::try_from(value) {
        Ok(pb::StopReason::EndTurn) => Ok(AgentStopReason::EndTurn),
        Ok(pb::StopReason::OutputLimit) => Ok(AgentStopReason::OutputLimit),
        Ok(pb::StopReason::TransferUnresolved) => Ok(AgentStopReason::TransferUnresolved),
        _ => Err(invalid()),
    }
}
