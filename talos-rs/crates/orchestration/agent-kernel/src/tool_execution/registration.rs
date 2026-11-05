//! Invocation-local registration policy over the existing trusted control port.
use std::sync::Arc;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_tools::ToolContext;
use talos_types::{
    AgentCaller, AgentUserContent, AgentUserInput, AutomationAction, AutomationMode,
    AutomationModelRequest, AutomationTurnOrigin, GoalAction, LoopAction, SkillSelector,
};
use tokio::sync::OnceCell;

#[derive(Debug)]
pub(crate) struct RegistrationGuard {
    mode: AutomationMode,
    registration_only: OnceCell<TalosResult<bool>>,
}

impl RegistrationGuard {
    pub(crate) fn from_initial(
        input: &str,
        selected: &[SkillSelector],
    ) -> TalosResult<Option<Arc<Self>>> {
        let mut content = vec![AgentUserContent::Text { text: input.into() }];
        content.extend(selected.iter().filter_map(|skill| {
            skill
                .name
                .as_ref()
                .map(|name| AgentUserContent::SkillSelection {
                    name: name.clone(),
                    path: skill
                        .path
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                })
        }));
        Ok(
            talos_types::explicit_automation_mode(&AgentUserInput { content })?.map(|mode| {
                Arc::new(Self {
                    mode,
                    registration_only: OnceCell::new(),
                })
            }),
        )
    }

    pub(crate) async fn resolve(&self, context: &ToolContext, call_id: &str) -> TalosResult<bool> {
        let registration_only = self
            .registration_only
            .get_or_init(|| async {
                let control = context.automation_control().ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Automation registration authorization is unavailable.",
                    )
                })?;
                let turn_id = context.turn_id().filter(|id| !id.trim().is_empty());
                let agent_id = context.agent_id().filter(|id| !id.trim().is_empty());
                let caller = match (turn_id, agent_id) {
                    (Some(turn_id), Some(agent_id)) if !context.session_id().trim().is_empty() => {
                        AgentCaller {
                            session_id: context.session_id().into(),
                            turn_id: turn_id.into(),
                            agent_id: agent_id.into(),
                        }
                    }
                    _ => {
                        return Err(TalosError::new(
                            ErrorCode::PermissionDenied,
                            "Automation registration requires a trusted Turn.",
                        ))
                    }
                };
                if call_id.trim().is_empty() {
                    return Err(TalosError::new(
                        ErrorCode::PermissionDenied,
                        "Automation registration requires a trusted Tool call.",
                    ));
                }
                let result = control
                    .execute(AutomationModelRequest {
                        caller: caller.clone(),
                        call_id: call_id.to_owned(),
                        action: match self.mode {
                            AutomationMode::Goal => AutomationAction::Goal {
                                action: GoalAction::Read {},
                            },
                            AutomationMode::Loop => AutomationAction::Loop {
                                action: LoopAction::List {},
                            },
                        },
                    })
                    .await?;
                match (self.mode, result.bound_origin) {
                    (_, None) => Ok(true),
                    (AutomationMode::Loop, Some(AutomationTurnOrigin::Loop { .. })) => Ok(false),
                    (AutomationMode::Goal, Some(AutomationTurnOrigin::Goal { goal_id, .. })) => {
                        let goal = result
                            .snapshot
                            .goal
                            .filter(|goal| goal.id == goal_id)
                            .ok_or_else(|| {
                                TalosError::new(
                                    ErrorCode::Conflict,
                                    "The Goal registration binding is unavailable.",
                                )
                            })?;
                        Ok(goal.latest_turn_id.as_deref() != Some(caller.turn_id.as_str()))
                    }
                    _ => Err(TalosError::new(
                        ErrorCode::PermissionDenied,
                        "Automation mode does not match this Turn's binding.",
                    )),
                }
            })
            .await
            .as_ref()
            .map_err(Clone::clone)?;
        Ok(*registration_only)
    }

    /// Deterministic request facts, separate from loaded Skill instructions.
    pub(crate) fn runtime_context(&self) -> TalosResult<String> {
        let registration_only = self
            .registration_only
            .get()
            .ok_or_else(|| {
                TalosError::internal(
                    "Automation context was requested before trusted classification.",
                )
            })?
            .as_ref()
            .map_err(Clone::clone)?;
        let request = if *registration_only {
            "explicit_activation"
        } else {
            "bound_automatic_work"
        };
        Ok(format!(
            "[AutomationInvocation]\nBackend-confirmed facts for this original request:\n- selected_capability: {}\n- current_request: {request}\n- activation_requested: {registration_only}\n- registration_only: {registration_only}\n{}",
            self.mode.skill_name(),
            if *registration_only {
                "The original user request explicitly invokes this capability. Task completion conditions are task content, not a request to manage an old completed task. Follow the Skill's activation instructions."
            } else {
                "This Turn is already bound to automatic work. The selected Skill supplies its instructions; it is not a new activation request."
            }
        ))
    }

    pub(crate) async fn check(
        &self,
        context: &ToolContext,
        call_id: &str,
        tool_name: &str,
    ) -> TalosResult<()> {
        let registration_only = self.resolve(context, call_id).await?;
        if registration_only && !matches!(tool_name, "automation" | "AskUserQuestion") {
            return Err(TalosError::new(ErrorCode::PermissionDenied, "This Turn only registers automation or asks for missing instructions. Task execution starts after the cancellation window."));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "registration_tests.rs"]
mod tests;
