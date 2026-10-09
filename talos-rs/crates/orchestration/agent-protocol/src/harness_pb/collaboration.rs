use std::str::FromStr;

use agent_pb as pb;
use talos_error::TalosResult;
use talos_types::{
    AgentPath, AgentTreeNode, AgentTreeSnapshot, CollaborationToolResult, SendMessageResult,
    SubagentOutcome, SubagentRef, SubagentSnapshot, SubagentSpawnResult, SubagentUpdate,
    SubagentUpdateKind,
};

use super::context::{decode_turn_ref, encode_turn_ref};
use super::invalid;

pub fn encode_collaboration(value: CollaborationToolResult) -> pb::CollaborationResult {
    use pb::collaboration_result::Result;
    pb::CollaborationResult {
        result: Some(match value {
            CollaborationToolResult::SpawnAgent(value) => {
                Result::SpawnAgent(pb::SubagentSpawnResult {
                    subagent: Some(encode_ref(value.subagent)),
                    agent_id: value.agent_id,
                    turn: Some(encode_turn_ref(value.turn)),
                })
            }
            CollaborationToolResult::SendMessage(value) => {
                Result::SendMessage(pb::SendMessageResult {
                    subagent: Some(encode_ref(value.subagent)),
                    agent_id: value.agent_id,
                    turn: Some(encode_turn_ref(value.turn)),
                    accepted: value.accepted,
                })
            }
            CollaborationToolResult::WaitAgent(value) => Result::WaitAgent(encode_snapshot(value)),
            CollaborationToolResult::InterruptAgent(value) => {
                Result::InterruptAgent(encode_snapshot(value))
            }
            CollaborationToolResult::FollowupTask(value) => {
                Result::FollowupTask(encode_snapshot(value))
            }
            CollaborationToolResult::ListAgents(value) => Result::ListAgents(pb::AgentTree {
                children: value.children.into_iter().map(encode_tree_node).collect(),
            }),
        }),
    }
}

pub fn decode_collaboration(
    value: pb::CollaborationResult,
) -> TalosResult<CollaborationToolResult> {
    use pb::collaboration_result::Result;
    match value.result.ok_or_else(invalid)? {
        Result::SpawnAgent(value) => Ok(CollaborationToolResult::SpawnAgent(SubagentSpawnResult {
            subagent: decode_ref(value.subagent)?,
            agent_id: value.agent_id,
            turn: decode_turn_ref(value.turn)?,
        })),
        Result::SendMessage(value) => Ok(CollaborationToolResult::SendMessage(SendMessageResult {
            subagent: decode_ref(value.subagent)?,
            agent_id: value.agent_id,
            turn: decode_turn_ref(value.turn)?,
            accepted: value.accepted,
        })),
        Result::WaitAgent(value) => Ok(CollaborationToolResult::WaitAgent(decode_snapshot(value)?)),
        Result::InterruptAgent(value) => Ok(CollaborationToolResult::InterruptAgent(
            decode_snapshot(value)?,
        )),
        Result::FollowupTask(value) => Ok(CollaborationToolResult::FollowupTask(decode_snapshot(
            value,
        )?)),
        Result::ListAgents(value) => Ok(CollaborationToolResult::ListAgents(AgentTreeSnapshot {
            children: value
                .children
                .into_iter()
                .map(decode_tree_node)
                .collect::<TalosResult<_>>()?,
        })),
    }
}

fn encode_ref(value: SubagentRef) -> pb::SubagentRef {
    pb::SubagentRef {
        path: value.path.as_str().into(),
        session_id: value.session_id,
        parent: Some(encode_turn_ref(value.parent)),
    }
}

fn decode_ref(value: Option<pb::SubagentRef>) -> TalosResult<SubagentRef> {
    let value = value.ok_or_else(invalid)?;
    Ok(SubagentRef {
        path: AgentPath::from_str(&value.path)?,
        session_id: value.session_id,
        parent: decode_turn_ref(value.parent)?,
    })
}

fn encode_snapshot(value: SubagentSnapshot) -> pb::SubagentSnapshot {
    use pb::subagent_snapshot::Outcome;
    pb::SubagentSnapshot {
        subagent: Some(encode_ref(value.subagent)),
        agent_id: value.agent_id,
        latest_update: value.latest_update.map(|value| pb::SubagentUpdate {
            kind: match value.kind {
                SubagentUpdateKind::Commentary => pb::SubagentUpdateKind::Commentary as i32,
                SubagentUpdateKind::FinalAnswer => pb::SubagentUpdateKind::FinalAnswer as i32,
            },
            text: value.text,
            truncated: value.truncated,
        }),
        outcome: Some(match value.outcome {
            SubagentOutcome::Running { turn_id } => Outcome::RunningTurnId(turn_id),
            SubagentOutcome::WaitingUserAction { turn_id } => {
                Outcome::WaitingUserActionTurnId(turn_id)
            }
            SubagentOutcome::Cancelling { turn_id } => Outcome::CancellingTurnId(turn_id),
            SubagentOutcome::Completed {
                turn_id,
                output,
                truncated,
            } => Outcome::Completed(pb::ChildCompleted {
                turn_id,
                output,
                truncated,
            }),
            SubagentOutcome::Failed {
                turn_id,
                error,
                truncated,
            } => Outcome::Failed(pb::ChildFailed {
                turn_id,
                error,
                truncated,
            }),
            SubagentOutcome::Interrupted { turn_id } => Outcome::InterruptedTurnId(turn_id),
        }),
    }
}

fn decode_snapshot(value: pb::SubagentSnapshot) -> TalosResult<SubagentSnapshot> {
    use pb::subagent_snapshot::Outcome;
    let outcome = match value.outcome.ok_or_else(invalid)? {
        Outcome::RunningTurnId(turn_id) => SubagentOutcome::Running { turn_id },
        Outcome::WaitingUserActionTurnId(turn_id) => SubagentOutcome::WaitingUserAction { turn_id },
        Outcome::CancellingTurnId(turn_id) => SubagentOutcome::Cancelling { turn_id },
        Outcome::Completed(value) => SubagentOutcome::Completed {
            turn_id: value.turn_id,
            output: value.output,
            truncated: value.truncated,
        },
        Outcome::Failed(value) => SubagentOutcome::Failed {
            turn_id: value.turn_id,
            error: value.error,
            truncated: value.truncated,
        },
        Outcome::InterruptedTurnId(turn_id) => SubagentOutcome::Interrupted { turn_id },
    };
    let latest_update = value
        .latest_update
        .map(|value| -> TalosResult<SubagentUpdate> {
            let kind = match pb::SubagentUpdateKind::try_from(value.kind) {
                Ok(pb::SubagentUpdateKind::Commentary) => SubagentUpdateKind::Commentary,
                Ok(pb::SubagentUpdateKind::FinalAnswer) => SubagentUpdateKind::FinalAnswer,
                _ => return Err(invalid()),
            };
            Ok(SubagentUpdate {
                kind,
                text: value.text,
                truncated: value.truncated,
            })
        })
        .transpose()?;
    Ok(SubagentSnapshot {
        subagent: decode_ref(value.subagent)?,
        agent_id: value.agent_id,
        latest_update,
        outcome,
    })
}

fn encode_tree_node(value: AgentTreeNode) -> pb::AgentTreeNode {
    pb::AgentTreeNode {
        snapshot: Some(encode_snapshot(value.snapshot)),
        children: value.children.into_iter().map(encode_tree_node).collect(),
    }
}

fn decode_tree_node(value: pb::AgentTreeNode) -> TalosResult<AgentTreeNode> {
    Ok(AgentTreeNode {
        snapshot: decode_snapshot(value.snapshot.ok_or_else(invalid)?)?,
        children: value
            .children
            .into_iter()
            .map(decode_tree_node)
            .collect::<TalosResult<_>>()?,
    })
}
