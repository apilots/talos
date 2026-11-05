//! Trusted model intent and bound-object authorization for the original action owner.
use super::*;
impl SessionAutomationService {
    pub(super) async fn validate_bound_action(
        &self,
        caller: &AgentCaller,
        action: &AutomationAction,
    ) -> TalosResult<()> {
        let Some(origin) = self.store.origin(&caller.turn_ref()).await? else {
            return Ok(());
        };
        if matches!(
            action,
            AutomationAction::Goal {
                action: GoalAction::Read {}
            } | AutomationAction::Loop {
                action: LoopAction::List {}
            }
        ) {
            return Ok(());
        }
        let user_steered = self.store.has_user_steer(&caller.turn_ref()).await?;
        let allowed = match (origin, action) {
            (
                AutomationTurnOrigin::Goal {
                    goal_id,
                    objective_revision,
                },
                AutomationAction::Goal { action },
            ) => {
                if !user_steered
                    && !matches!(
                        action,
                        GoalAction::Create { .. }
                            | GoalAction::Pause { .. }
                            | GoalAction::Complete { .. }
                    )
                {
                    return Err(TalosError::new(ErrorCode::PermissionDenied,"Automatic Goal Turns cannot edit, clear or resume user-managed definitions."));
                }
                if matches!(action, GoalAction::Create { .. }) {
                    true
                } else {
                    let target = match action {
                        GoalAction::Edit { goal_id, .. }
                        | GoalAction::Pause { goal_id }
                        | GoalAction::Resume { goal_id }
                        | GoalAction::Clear { goal_id }
                        | GoalAction::Complete { goal_id, .. } => goal_id,
                        _ => return Err(conflict()),
                    };
                    target == &goal_id
                        && self.store.goal(target).await?.objective_revision == objective_revision
                }
            }
            (
                AutomationTurnOrigin::Loop {
                    loop_id,
                    config_revision,
                    ..
                },
                AutomationAction::Loop { action },
            ) => {
                if !user_steered && !matches!(action, LoopAction::Decide { .. }) {
                    return Err(TalosError::new(ErrorCode::PermissionDenied,"Automatic Loop Turns may only decide the next model-decided occurrence; stopping or changing user-managed definitions requires user input."));
                }
                let target = match action {
                    LoopAction::Edit { loop_id, .. }
                    | LoopAction::Stop { loop_id }
                    | LoopAction::Clear { loop_id }
                    | LoopAction::Decide { loop_id, .. } => loop_id,
                    _ => {
                        return Err(TalosError::new(
                            ErrorCode::PermissionDenied,
                            "Automatic Turns cannot create further automation.",
                        ))
                    }
                };
                target == &loop_id
                    && self.store.loop_task(target).await?.config_revision == config_revision
            }
            _ => false,
        };
        if !allowed {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "Model automation mutations must match the current bound object and revision.",
            ));
        }
        Ok(())
    }
    pub(super) async fn require_creation(
        &self,
        session: &SessionRecord,
        mode: AutomationMode,
        caller: Option<&AgentCaller>,
    ) -> TalosResult<()> {
        if let Some(caller) = caller {
            if self.store.origin(&caller.turn_ref()).await?.is_some() {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Automatic Turns cannot create further automation.",
                ));
            }
            // Plain User requests are judged by the model using the shipped Skills.
            // A typed/leading-command selection still constrains its chosen mode.
            if explicit_automation_mode(&self.initial_input(&caller.turn_ref()).await?)?
                .is_some_and(|selected| selected != mode)
            {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Automation creation must match the explicitly selected mode.",
                ));
            }
        } else if self
            .execution
            .get_agent_active_turn(&session.id)
            .await?
            .is_some()
            || !self.queue.list(&session.id)?.messages.is_empty()
        {
            return Err(conflict());
        }
        self.validate_mode(&session.id, mode, caller.map(|c| c.turn_id.as_str()), None)
            .await
    }
}
