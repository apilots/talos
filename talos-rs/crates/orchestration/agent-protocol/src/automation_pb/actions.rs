use super::{invalid, required};
use agent_pb as pb;
use talos_error::TalosResult;
use talos_types::{AutomationAction, GoalAction, LoopAction, LoopSchedule, LoopWakeDecision};

pub(super) fn encode_action(value: AutomationAction) -> pb::AutomationAction {
    use pb::automation_action::Target;
    let target = match value {
        AutomationAction::Goal { action } => Target::Goal(pb::AutomationGoalAction {
            action: Some(encode_goal(action)),
        }),
        AutomationAction::Loop { action } => Target::LoopTask(pb::AutomationLoopAction {
            action: Some(encode_loop(action)),
        }),
    };
    pb::AutomationAction {
        target: Some(target),
    }
}

pub(super) fn decode_action(value: pb::AutomationAction) -> TalosResult<AutomationAction> {
    Ok(match value.target.ok_or_else(invalid)? {
        pb::automation_action::Target::Goal(value) => AutomationAction::Goal {
            action: decode_goal(value.action.ok_or_else(invalid)?)?,
        },
        pb::automation_action::Target::LoopTask(value) => AutomationAction::Loop {
            action: decode_loop(value.action.ok_or_else(invalid)?)?,
        },
    })
}

fn encode_goal(value: GoalAction) -> pb::automation_goal_action::Action {
    use pb::automation_goal_action::Action;
    match value {
        GoalAction::Create {
            objective,
            token_budget,
        } => Action::Create(pb::CreateAutomationGoal {
            objective,
            token_budget,
        }),
        GoalAction::Read {} => Action::Read(pb::Empty {}),
        GoalAction::Edit {
            goal_id,
            expected_revision,
            objective,
        } => Action::Edit(pb::EditAutomationGoal {
            goal_id,
            expected_revision,
            objective,
        }),
        GoalAction::Pause { goal_id } => Action::Pause(pb::AutomationGoalId { goal_id }),
        GoalAction::Resume { goal_id } => Action::Resume(pb::AutomationGoalId { goal_id }),
        GoalAction::Clear { goal_id } => Action::Clear(pb::AutomationGoalId { goal_id }),
        GoalAction::Complete {
            goal_id,
            objective_revision,
            evidence,
        } => Action::Complete(pb::CompleteAutomationGoal {
            goal_id,
            objective_revision,
            evidence,
        }),
    }
}

fn decode_goal(value: pb::automation_goal_action::Action) -> TalosResult<GoalAction> {
    use pb::automation_goal_action::Action;
    Ok(match value {
        Action::Create(value) => GoalAction::Create {
            objective: value.objective,
            token_budget: value.token_budget,
        },
        Action::Read(_) => GoalAction::Read {},
        Action::Edit(value) => {
            required(&value.goal_id)?;
            GoalAction::Edit {
                goal_id: value.goal_id,
                expected_revision: value.expected_revision,
                objective: value.objective,
            }
        }
        Action::Pause(value) => {
            required(&value.goal_id)?;
            GoalAction::Pause {
                goal_id: value.goal_id,
            }
        }
        Action::Resume(value) => {
            required(&value.goal_id)?;
            GoalAction::Resume {
                goal_id: value.goal_id,
            }
        }
        Action::Clear(value) => {
            required(&value.goal_id)?;
            GoalAction::Clear {
                goal_id: value.goal_id,
            }
        }
        Action::Complete(value) => {
            required(&value.goal_id)?;
            GoalAction::Complete {
                goal_id: value.goal_id,
                objective_revision: value.objective_revision,
                evidence: value.evidence,
            }
        }
    })
}

fn encode_loop(value: LoopAction) -> pb::automation_loop_action::Action {
    use pb::automation_loop_action::Action;
    match value {
        LoopAction::Create { prompt, schedule } => Action::Create(pb::CreateAutomationLoop {
            prompt,
            schedule: Some(encode_schedule(schedule)),
        }),
        LoopAction::List {} => Action::List(pb::Empty {}),
        LoopAction::Edit {
            loop_id,
            expected_revision,
            prompt,
            schedule,
        } => Action::Edit(pb::EditAutomationLoop {
            loop_id,
            expected_revision,
            prompt,
            schedule: Some(encode_schedule(schedule)),
        }),
        LoopAction::Stop { loop_id } => Action::Stop(pb::AutomationLoopId { loop_id }),
        LoopAction::Clear { loop_id } => Action::Clear(pb::AutomationLoopId { loop_id }),
        LoopAction::Decide {
            loop_id,
            config_revision,
            decision,
        } => Action::Decide(pb::DecideAutomationLoop {
            loop_id,
            config_revision,
            decision: Some(encode_decision(decision)),
        }),
    }
}

fn decode_loop(value: pb::automation_loop_action::Action) -> TalosResult<LoopAction> {
    use pb::automation_loop_action::Action;
    Ok(match value {
        Action::Create(value) => LoopAction::Create {
            prompt: value.prompt,
            schedule: decode_schedule(value.schedule)?,
        },
        Action::List(_) => LoopAction::List {},
        Action::Edit(value) => {
            required(&value.loop_id)?;
            LoopAction::Edit {
                loop_id: value.loop_id,
                expected_revision: value.expected_revision,
                prompt: value.prompt,
                schedule: decode_schedule(value.schedule)?,
            }
        }
        Action::Stop(value) => {
            required(&value.loop_id)?;
            LoopAction::Stop {
                loop_id: value.loop_id,
            }
        }
        Action::Clear(value) => {
            required(&value.loop_id)?;
            LoopAction::Clear {
                loop_id: value.loop_id,
            }
        }
        Action::Decide(value) => {
            required(&value.loop_id)?;
            LoopAction::Decide {
                loop_id: value.loop_id,
                config_revision: value.config_revision,
                decision: decode_decision(value.decision)?,
            }
        }
    })
}

pub(super) fn encode_schedule(value: LoopSchedule) -> pb::AutomationLoopSchedule {
    use pb::automation_loop_schedule::Schedule;
    pb::AutomationLoopSchedule {
        schedule: Some(match value {
            LoopSchedule::FixedInterval { minutes } => Schedule::FixedMinutes(minutes),
            LoopSchedule::ModelDecided {} => Schedule::ModelDecided(pb::Empty {}),
        }),
    }
}

pub(super) fn decode_schedule(
    value: Option<pb::AutomationLoopSchedule>,
) -> TalosResult<LoopSchedule> {
    let schedule = match value.and_then(|value| value.schedule).ok_or_else(invalid)? {
        pb::automation_loop_schedule::Schedule::FixedMinutes(minutes) => {
            LoopSchedule::FixedInterval { minutes }
        }
        pb::automation_loop_schedule::Schedule::ModelDecided(_) => LoopSchedule::ModelDecided {},
    };
    schedule.validate()?;
    Ok(schedule)
}

fn encode_decision(value: LoopWakeDecision) -> pb::AutomationLoopDecision {
    use pb::automation_loop_decision::Decision;
    pb::AutomationLoopDecision {
        decision: Some(match value {
            LoopWakeDecision::Continue {
                delay_minutes,
                reason,
            } => Decision::ContinueLoop(pb::ContinueAutomationLoop {
                delay_minutes,
                reason,
            }),
            LoopWakeDecision::Stop { reason } => Decision::StopReason(reason),
        }),
    }
}

fn decode_decision(value: Option<pb::AutomationLoopDecision>) -> TalosResult<LoopWakeDecision> {
    Ok(
        match value.and_then(|value| value.decision).ok_or_else(invalid)? {
            pb::automation_loop_decision::Decision::ContinueLoop(value) => {
                if value.delay_minutes == 0 {
                    return Err(invalid());
                }
                LoopWakeDecision::Continue {
                    delay_minutes: value.delay_minutes,
                    reason: value.reason,
                }
            }
            pb::automation_loop_decision::Decision::StopReason(reason) => {
                LoopWakeDecision::Stop { reason }
            }
        },
    )
}
