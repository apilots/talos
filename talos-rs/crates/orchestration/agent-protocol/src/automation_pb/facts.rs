use super::{invalid, required};
use agent_pb as pb;
use talos_error::TalosResult;
use talos_types::{
    AutomationEffect, AutomationTurnOrigin, GoalPauseReason, GoalStatus, LoopStatus,
    LoopStopReason, SessionAutomationSnapshot, SessionGoal, SessionLoop,
};

macro_rules! enum_codec {
    ($encode:ident, $decode:ident, $domain:ident, $wire:ident, [$($variant:ident),+]) => {
        pub(super) fn $encode(value: $domain) -> i32 {
            match value { $($domain::$variant => pb::$wire::$variant as i32),+ }
        }
        pub(super) fn $decode(value: i32) -> TalosResult<$domain> {
            match pb::$wire::try_from(value) {
                $(Ok(pb::$wire::$variant) => Ok($domain::$variant),)+
                Ok(pb::$wire::Unspecified) | Err(_) => Err(invalid()),
            }
        }
    };
}
enum_codec!(
    encode_effect,
    decode_effect,
    AutomationEffect,
    AutomationEffect,
    [Applied, PendingSettlement]
);
enum_codec!(
    encode_goal_status,
    decode_goal_status,
    GoalStatus,
    AutomationGoalStatus,
    [Active, Paused, Complete, Cleared]
);
enum_codec!(
    encode_pause_reason,
    decode_pause_reason,
    GoalPauseReason,
    AutomationGoalPauseReason,
    [
        UserPaused,
        UserCancelled,
        WaitingForUser,
        ExecutionFailed,
        BudgetExhausted,
        NoProgress
    ]
);
enum_codec!(
    encode_loop_status,
    decode_loop_status,
    LoopStatus,
    AutomationLoopStatus,
    [Active, Stopped]
);
enum_codec!(
    encode_stop_reason,
    decode_stop_reason,
    LoopStopReason,
    AutomationLoopStopReason,
    [
        UserStopped,
        UserCancelled,
        ModelStopped,
        WaitingForUser,
        ExecutionFailed,
        MissingDecision,
        QueueFull
    ]
);

pub(super) fn encode_origin(value: AutomationTurnOrigin) -> pb::AutomationTurnOrigin {
    use pb::automation_turn_origin::Origin;
    pb::AutomationTurnOrigin {
        origin: Some(match value {
            AutomationTurnOrigin::Goal {
                goal_id,
                objective_revision,
            } => Origin::Goal(pb::AutomationGoalOrigin {
                goal_id,
                objective_revision,
            }),
            AutomationTurnOrigin::Loop {
                loop_id,
                occurrence_id,
                config_revision,
            } => Origin::LoopTask(pb::AutomationLoopOrigin {
                loop_id,
                occurrence_id,
                config_revision,
            }),
        }),
    }
}

pub(super) fn decode_origin(value: pb::AutomationTurnOrigin) -> TalosResult<AutomationTurnOrigin> {
    Ok(match value.origin.ok_or_else(invalid)? {
        pb::automation_turn_origin::Origin::Goal(value) => {
            required(&value.goal_id)?;
            AutomationTurnOrigin::Goal {
                goal_id: value.goal_id,
                objective_revision: value.objective_revision,
            }
        }
        pb::automation_turn_origin::Origin::LoopTask(value) => {
            required(&value.loop_id)?;
            required(&value.occurrence_id)?;
            AutomationTurnOrigin::Loop {
                loop_id: value.loop_id,
                occurrence_id: value.occurrence_id,
                config_revision: value.config_revision,
            }
        }
    })
}

pub(super) fn encode_snapshot(value: SessionAutomationSnapshot) -> pb::AutomationSessionSnapshot {
    pb::AutomationSessionSnapshot {
        session_id: value.session_id,
        goal: value.goal.map(encode_goal),
        loops: value.loops.into_iter().map(encode_loop).collect(),
    }
}

pub(super) fn decode_snapshot(
    value: pb::AutomationSessionSnapshot,
) -> TalosResult<SessionAutomationSnapshot> {
    required(&value.session_id)?;
    let goal = value.goal.map(decode_goal).transpose()?;
    let loops = value
        .loops
        .into_iter()
        .map(decode_loop)
        .collect::<TalosResult<Vec<_>>>()?;
    if goal
        .as_ref()
        .is_some_and(|goal| goal.session_id != value.session_id)
        || loops.iter().any(|task| task.session_id != value.session_id)
    {
        return Err(invalid());
    }
    Ok(SessionAutomationSnapshot {
        session_id: value.session_id,
        goal,
        loops,
    })
}

fn encode_goal(value: SessionGoal) -> pb::AutomationGoal {
    pb::AutomationGoal {
        id: value.id,
        session_id: value.session_id,
        objective: value.objective,
        objective_revision: value.objective_revision,
        status: encode_goal_status(value.status),
        pause_reason: value.pause_reason.map(encode_pause_reason),
        reason: value.reason,
        token_budget: value.token_budget,
        tokens_used: value.tokens_used,
        latest_turn_id: value.latest_turn_id,
        completion_evidence: value.completion_evidence,

        created_at: value.created_at,
        updated_at: value.updated_at,
    }
}

fn decode_goal(value: pb::AutomationGoal) -> TalosResult<SessionGoal> {
    required(&value.id)?;
    required(&value.session_id)?;
    if let Some(turn) = &value.latest_turn_id {
        required(turn)?;
    }
    Ok(SessionGoal {
        id: value.id,
        session_id: value.session_id,
        objective: value.objective,
        objective_revision: value.objective_revision,
        status: decode_goal_status(value.status)?,
        pause_reason: value.pause_reason.map(decode_pause_reason).transpose()?,
        reason: value.reason,
        token_budget: value.token_budget,
        tokens_used: value.tokens_used,
        latest_turn_id: value.latest_turn_id,
        completion_evidence: value.completion_evidence,

        created_at: value.created_at,
        updated_at: value.updated_at,
    })
}

fn encode_loop(value: SessionLoop) -> pb::AutomationLoop {
    pb::AutomationLoop {
        id: value.id,
        session_id: value.session_id,
        prompt: value.prompt,
        schedule: Some(super::actions::encode_schedule(value.schedule)),
        config_revision: value.config_revision,
        status: encode_loop_status(value.status),
        stop_reason: value.stop_reason.map(encode_stop_reason),
        reason: value.reason,
        next_due_at: value.next_due_at,
        latest_turn_id: value.latest_turn_id,
        created_at: value.created_at,
        updated_at: value.updated_at,
    }
}

fn decode_loop(value: pb::AutomationLoop) -> TalosResult<SessionLoop> {
    required(&value.id)?;
    required(&value.session_id)?;
    if let Some(turn) = &value.latest_turn_id {
        required(turn)?;
    }
    Ok(SessionLoop {
        id: value.id,
        session_id: value.session_id,
        prompt: value.prompt,
        schedule: super::actions::decode_schedule(value.schedule)?,
        config_revision: value.config_revision,
        status: decode_loop_status(value.status)?,
        stop_reason: value.stop_reason.map(decode_stop_reason).transpose()?,
        reason: value.reason,
        next_due_at: value.next_due_at,
        latest_turn_id: value.latest_turn_id,
        created_at: value.created_at,
        updated_at: value.updated_at,
    })
}
