//! Thin terminal presentation over the existing SDK command and Session owners.
use crate::{GatewayClient, TuiApp};
use std::time::Duration;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_sdk::*;

#[derive(Clone, Debug)]
pub(crate) enum EditTarget {
    Goal(SessionGoal),
    Loop(SessionLoop),
}
#[derive(Clone, Debug)]
pub(crate) struct AutomationEdit {
    pub(crate) target: EditTarget,
    pub(crate) schedule: Option<LoopSchedule>,
}

pub(crate) async fn system_skill(
    client: &dyn GatewayClient,
    session: Option<&str>,
    mode: AutomationMode,
) -> TalosResult<SkillSuggestion> {
    let node_id = match session {
        Some(id) => Some(client.get_session(id).await?.node_id),
        None => None,
    };
    let skills = client
        .list_skills(SkillsListRequest {
            node_id,
            include_disabled: false,
        })
        .await?;
    let mut found = skills.data.iter().filter(|skill| {
        skill.name == mode.skill_name()
            && skill.scope == SkillScope::System
            && skill.enabled
            && skill.read_only
    });
    let skill = found.next().ok_or_else(|| {
        TalosError::new(
            ErrorCode::ResourceNotFound,
            "Protected System Skill is unavailable on this Computer.",
        )
    })?;
    if found.next().is_some() {
        return Err(TalosError::new(
            ErrorCode::Conflict,
            "System Skill selection is ambiguous.",
        ));
    }
    Ok(SkillSuggestion::from(skill))
}
impl TuiApp {
    pub(crate) async fn stage_automation_mode(&mut self, mode: AutomationMode) -> TalosResult<()> {
        let from_suggestion = self.show_command_autocomplete;
        let client = self
            .gateway_client
            .clone()
            .ok_or_else(|| TalosError::new(ErrorCode::ServiceUnavailable, "Gateway unavailable"))?;
        let skill = system_skill(client.as_ref(), self.session_id.as_deref(), mode).await?;
        self.selected_skills
            .retain(|skill| skill.name != "goal" && skill.name != "loop");
        self.selected_skills.push(skill);
        self.show_command_autocomplete = false;
        self.command_input_buffer.clear();
        if (from_suggestion && self.input_buffer.starts_with('/'))
            || self.input_buffer.trim() == format!("/{}", mode.skill_name())
            || self.input_buffer == "/"
        {
            self.input_buffer.clear();
            self.cursor_index = 0;
        }
        self.show_status_hint(
            format!(
                "{} selected · send a prompt; Ctrl+U clears",
                mode.skill_name()
            ),
            Duration::from_secs(3),
        );
        Ok(())
    }
    pub(crate) async fn refresh_automation(&mut self) {
        let (Some(client), Some(session_id)) =
            (self.gateway_client.clone(), self.session_id.clone())
        else {
            self.automation_snapshot = None;
            return;
        };
        match client
            .session_automation(SessionAutomationReadRequest {
                session_id: session_id.clone(),
            })
            .await
        {
            Ok(snapshot) => {
                self.automation_snapshot = Some(snapshot);
                self.automation_error = None;
                if self.automation_targets().is_empty() {
                    self.selected_automation = None;
                }
            }
            Err(error) => self.automation_error = Some(error.to_api_error().message),
        }
        // Metadata may change while the canonical event offset stays unchanged.
        match client.get_active_turn(&session_id).await {
            Ok(Some(active)) => {
                if active.initial_message_id.is_some() {
                    self.active_message
                        .get_or_insert_with(|| "Session task".into());
                }
                if let Some(input) = active.automation_input {
                    self.render_runtime
                        .set_automation_input(&active.identity.turn_id, input);
                    self.rendered_chat_cache_key = None;
                }
            }
            Ok(None) => {}
            Err(error) => self.push_right_event(format!(
                "automation input metadata: {}",
                error.to_api_error().message
            )),
        }
    }
    pub(crate) async fn apply_automation_action(
        &mut self,
        action: AutomationAction,
    ) -> TalosResult<()> {
        let client = self
            .gateway_client
            .clone()
            .ok_or_else(|| TalosError::new(ErrorCode::ServiceUnavailable, "Gateway unavailable"))?;
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| TalosError::new(ErrorCode::InvalidRequest, "Choose a Session first."))?;
        let result = client
            .execute_automation(AutomationRequest { session_id, action })
            .await?;
        self.automation_snapshot = Some(result.snapshot);
        self.automation_error = None;
        self.refresh_automation().await;
        self.refresh_message_queue(0).await;
        Ok(())
    }
    pub(crate) async fn handle_automation_command(&mut self, text: &str) -> TalosResult<()> {
        let client = self
            .gateway_client
            .clone()
            .ok_or_else(|| TalosError::new(ErrorCode::ServiceUnavailable, "Gateway unavailable"))?;
        let command = client
            .resolve_automation_command(AutomationCommandResolveRequest { text: text.into() })
            .await?
            .command
            .ok_or_else(|| {
                TalosError::new(ErrorCode::InvalidRequest, "Unknown automation command")
            })?;
        match command {
            AutomationCommand::Activate { mode, prompt } => {
                if self.selected_skills.iter().any(|skill| {
                    matches!(skill.name.as_str(), "goal" | "loop")
                        && skill.name != mode.skill_name()
                }) {
                    return Err(TalosError::new(
                        ErrorCode::InvalidRequest,
                        "The command and selected mode conflict. Clear or switch the mode first.",
                    ));
                }
                let skill = system_skill(client.as_ref(), self.session_id.as_deref(), mode).await?;
                let files = self.upload_pending_image_attachments().await?;
                let mut skills = self.selected_skills.clone();
                skills.retain(|entry| !matches!(entry.name.as_str(), "goal" | "loop"));
                skills.push(skill);
                if self
                    .send_user_message_with_skills(prompt, skills, files)
                    .await?
                {
                    self.reset_input();
                }
            }
            AutomationCommand::Read { mode } => {
                self.reset_input();
                self.refresh_automation().await;
                self.tasks_expanded = true;
                if self.automation_error.is_none() {
                    match (mode, self.automation_snapshot.as_ref()) {
                        (AutomationMode::Goal, Some(snapshot)) => {
                            let message = snapshot
                                .goal
                                .as_ref()
                                .map(|goal| {
                                    format!(
                                        "Goal {} · {:?} · {}{}",
                                        goal.id,
                                        goal.status,
                                        goal.objective,
                                        goal.completion_evidence
                                            .as_ref()
                                            .map(|evidence| format!("\nEvidence: {evidence}"))
                                            .unwrap_or_default()
                                    )
                                })
                                .unwrap_or_else(|| "No Goal in this Session.".into());
                            self.push_system_notice(message);
                        }
                        (AutomationMode::Loop, Some(snapshot)) => {
                            let message = if snapshot.loops.is_empty() {
                                "No active Loops in this Session.".into()
                            } else {
                                snapshot
                                    .loops
                                    .iter()
                                    .map(|task| format!("Loop {} · {}", task.id, task.prompt))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            };
                            self.push_system_notice(message);
                        }
                        _ => {}
                    }
                }
            }
            AutomationCommand::Edit { mode, object_id } => {
                self.refresh_automation().await;
                let snapshot = self.automation_snapshot.as_ref().ok_or_else(|| {
                    TalosError::new(ErrorCode::ResourceNotFound, "Automation state unavailable")
                })?;
                let target = match mode {
                    AutomationMode::Goal => {
                        EditTarget::Goal(snapshot.goal.clone().ok_or_else(|| {
                            TalosError::new(ErrorCode::ResourceNotFound, "No Goal in this Session")
                        })?)
                    }
                    AutomationMode::Loop => EditTarget::Loop(
                        snapshot
                            .loops
                            .iter()
                            .find(|task| Some(&task.id) == object_id.as_ref())
                            .cloned()
                            .ok_or_else(|| {
                                TalosError::new(
                                    ErrorCode::ResourceNotFound,
                                    "Active Loop not found",
                                )
                            })?,
                    ),
                };
                self.begin_automation_edit(target);
            }
            AutomationCommand::PauseGoal
            | AutomationCommand::ResumeGoal
            | AutomationCommand::ClearGoal => {
                self.refresh_automation().await;
                let id = self
                    .automation_snapshot
                    .as_ref()
                    .and_then(|s| s.goal.as_ref())
                    .map(|g| g.id.clone())
                    .ok_or_else(|| {
                        TalosError::new(ErrorCode::ResourceNotFound, "No Goal in this Session")
                    })?;
                let action = match command {
                    AutomationCommand::PauseGoal => GoalAction::Pause { goal_id: id },
                    AutomationCommand::ResumeGoal => GoalAction::Resume { goal_id: id },
                    _ => GoalAction::Clear { goal_id: id },
                };
                self.apply_automation_action(AutomationAction::Goal { action })
                    .await?;
                self.reset_input();
            }
            AutomationCommand::StopLoop { loop_id } => {
                self.apply_automation_action(AutomationAction::Loop {
                    action: LoopAction::Stop { loop_id },
                })
                .await?;
                self.reset_input();
            }
            AutomationCommand::ClearLoop { loop_id } => {
                self.apply_automation_action(AutomationAction::Loop {
                    action: LoopAction::Clear { loop_id },
                })
                .await?;
                self.reset_input();
            }
        }
        Ok(())
    }
    pub(crate) fn begin_automation_edit(&mut self, target: EditTarget) {
        if matches!(&target,EditTarget::Goal(goal) if !matches!(goal.status,GoalStatus::Active|GoalStatus::Paused))
        {
            self.show_status_hint(
                "Completed Goals can be inspected or cleared.",
                Duration::from_secs(3),
            );
            return;
        }
        self.reset_input();
        let (text, schedule) = match &target {
            EditTarget::Goal(g) => (g.objective.clone(), None),
            EditTarget::Loop(l) => (l.prompt.clone(), Some(l.schedule.clone())),
        };
        self.input_buffer = text;
        self.cursor_index = self.input_buffer.chars().count();
        self.automation_edit = Some(AutomationEdit { target, schedule });
        self.selected_automation = None;
        self.tasks_expanded = true;
    }
    pub(crate) async fn save_automation_edit(&mut self) -> TalosResult<()> {
        let edit = self
            .automation_edit
            .clone()
            .ok_or_else(|| TalosError::new(ErrorCode::InvalidRequest, "No edit draft"))?;
        let prompt = self.compose_user_input_text()?;
        if prompt.trim().is_empty() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Prompt cannot be empty",
            ));
        }
        let action = match edit.target {
            EditTarget::Goal(g) => AutomationAction::Goal {
                action: GoalAction::Edit {
                    goal_id: g.id,
                    expected_revision: g.objective_revision,
                    objective: prompt,
                },
            },
            EditTarget::Loop(l) => AutomationAction::Loop {
                action: LoopAction::Edit {
                    loop_id: l.id,
                    expected_revision: l.config_revision,
                    prompt,
                    schedule: edit.schedule.ok_or_else(|| {
                        TalosError::new(ErrorCode::InvalidRequest, "Loop schedule missing")
                    })?,
                },
            },
        };
        self.apply_automation_action(action).await?;
        self.automation_edit = None;
        self.reset_input();
        Ok(())
    }
    pub(crate) fn automation_targets(&self) -> Vec<EditTarget> {
        let Some(snapshot) = &self.automation_snapshot else {
            return vec![];
        };
        let mut targets = vec![];
        if let Some(goal) = &snapshot.goal {
            if matches!(goal.status, GoalStatus::Active | GoalStatus::Paused) {
                targets.push(EditTarget::Goal(goal.clone()));
            }
        }
        targets.extend(
            snapshot
                .loops
                .iter()
                .filter(|l| l.status == LoopStatus::Active)
                .cloned()
                .map(EditTarget::Loop),
        );
        targets
    }
    pub(crate) async fn selected_automation_action(&mut self, key: char) -> TalosResult<()> {
        let target = self
            .selected_automation
            .and_then(|i| self.automation_targets().get(i).cloned())
            .ok_or_else(|| TalosError::new(ErrorCode::ResourceNotFound, "No selected task"))?;
        if key == 'e' {
            self.begin_automation_edit(target);
            return Ok(());
        }
        let action = match target {
            EditTarget::Goal(g) => AutomationAction::Goal {
                action: match key {
                    'd' => GoalAction::Clear { goal_id: g.id },
                    _ if g.status == GoalStatus::Paused => GoalAction::Resume { goal_id: g.id },
                    _ if g.status == GoalStatus::Active => GoalAction::Pause { goal_id: g.id },
                    _ => return Ok(()),
                },
            },
            EditTarget::Loop(l) => AutomationAction::Loop {
                action: if key == 'd' {
                    LoopAction::Clear { loop_id: l.id }
                } else {
                    LoopAction::Stop { loop_id: l.id }
                },
            },
        };
        self.apply_automation_action(action).await?;
        self.selected_automation = None;
        Ok(())
    }
}

mod input;
mod panel;
mod timing;

pub mod foreground;
