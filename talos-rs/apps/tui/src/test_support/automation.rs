//! Explicit stateful fixtures for the terminal automation adapter.
use super::*;
use talos_sdk::*;
impl MockGatewayClient {
    pub(super) fn automation_snapshot(&self, session: &str) -> SessionAutomationSnapshot {
        self.state
            .lock()
            .unwrap()
            .automation
            .clone()
            .unwrap_or_else(|| SessionAutomationSnapshot {
                session_id: session.into(),
                goal: None,
                loops: vec![],
            })
    }
    pub(super) fn automation_list(
        &self,
        request: AutomationListRequest,
    ) -> TalosResult<AutomationListResponse> {
        let snapshot =
            self.automation_snapshot(request.session_id.as_deref().unwrap_or("sess-test"));
        let mut data = vec![];
        if let Some(goal) = snapshot.goal {
            data.push(AutomationListEntry::Goal { goal });
        }
        data.extend(
            snapshot
                .loops
                .into_iter()
                .map(|loop_task| AutomationListEntry::Loop { loop_task }),
        );
        data.retain(|entry| {
            let (session, ended, cleared) = match entry {
                AutomationListEntry::Goal { goal } => (
                    goal.session_id.as_str(),
                    goal.status == GoalStatus::Complete,
                    goal.status == GoalStatus::Cleared,
                ),
                AutomationListEntry::Loop { loop_task } => (
                    loop_task.session_id.as_str(),
                    loop_task.status != LoopStatus::Active,
                    false,
                ),
            };
            !cleared
                && request.session_id.as_deref().is_none_or(|id| id == session)
                && ended == (request.scope == AutomationListScope::Ended)
        });
        Ok(AutomationListResponse {
            data,
            next_cursor: None,
        })
    }
    pub(super) fn apply_automation(
        &self,
        request: AutomationRequest,
    ) -> TalosResult<AutomationControlResult> {
        let mut state = self.state.lock().unwrap();
        state.automation_requests.push(request.clone());
        let snapshot = state
            .automation
            .get_or_insert_with(|| SessionAutomationSnapshot {
                session_id: request.session_id.clone(),
                goal: None,
                loops: vec![],
            });
        match request.action {
            AutomationAction::Goal { action } => {
                let goal = snapshot.goal.as_mut().ok_or_else(|| {
                    TalosError::new(ErrorCode::ResourceNotFound, "No fixture Goal")
                })?;
                match action {
                    GoalAction::Edit {
                        goal_id,
                        expected_revision,
                        objective,
                    } if goal.id == goal_id && goal.objective_revision == expected_revision => {
                        goal.objective = objective;
                        goal.objective_revision += 1;
                    }
                    GoalAction::Pause { goal_id } if goal.id == goal_id => {
                        goal.status = GoalStatus::Paused
                    }
                    GoalAction::Resume { goal_id } if goal.id == goal_id => {
                        goal.status = GoalStatus::Active
                    }
                    GoalAction::Clear { goal_id } if goal.id == goal_id => snapshot.goal = None,
                    GoalAction::Read {} => {}
                    _ => {
                        return Err(TalosError::new(
                            ErrorCode::Conflict,
                            "Unsupported fixture Goal action",
                        ))
                    }
                }
            }
            AutomationAction::Loop { action } => {
                let id = match &action {
                    LoopAction::Edit { loop_id, .. }
                    | LoopAction::Stop { loop_id }
                    | LoopAction::Clear { loop_id } => loop_id,
                    _ => {
                        return Err(TalosError::new(
                            ErrorCode::InvalidRequest,
                            "Unsupported fixture Loop action",
                        ))
                    }
                };
                let task = snapshot
                    .loops
                    .iter_mut()
                    .find(|task| &task.id == id)
                    .ok_or_else(|| {
                        TalosError::new(ErrorCode::ResourceNotFound, "No fixture Loop")
                    })?;
                match action {
                    LoopAction::Edit {
                        expected_revision,
                        prompt,
                        schedule,
                        ..
                    } if task.config_revision == expected_revision => {
                        schedule.validate()?;
                        task.prompt = prompt;
                        task.schedule = schedule;
                        task.config_revision += 1;
                    }
                    LoopAction::Stop { .. } => task.status = LoopStatus::Stopped,
                    LoopAction::Clear { loop_id } => {
                        snapshot.loops.retain(|task| task.id != loop_id)
                    }
                    _ => {
                        return Err(TalosError::new(
                            ErrorCode::Conflict,
                            "Fixture revision changed",
                        ))
                    }
                }
            }
        }
        Ok(AutomationControlResult {
            effect: AutomationEffect::Applied,
            created_object: None,
            bound_origin: None,
            snapshot: snapshot.clone(),
        })
    }
    pub(super) fn apply_builtin_job(
        &self,
        request: BuiltinJobControlRequest,
    ) -> TalosResult<BuiltinJobInfo> {
        let mut state = self.state.lock().unwrap();
        let job = state
            .builtin_jobs
            .iter_mut()
            .find(|j| {
                j.id == request.job_id
                    && j.computer_id == request.computer_id
                    && j.execution_instance_id == request.execution_instance_id
            })
            .ok_or_else(|| TalosError::new(ErrorCode::ResourceNotFound, "No fixture Job"))?;
        match request.action {
            BuiltinJobAction::Pause => job.paused = true,
            BuiltinJobAction::Resume => job.paused = false,
            BuiltinJobAction::RunOnce => job.running = true,
        }
        Ok(job.clone())
    }
    pub(super) fn system_skills(&self) -> TalosResult<SkillsListResponse> {
        Ok(SkillsListResponse {
            data: ["goal", "loop"]
                .into_iter()
                .map(|name| SkillSummary {
                    name: name.into(),
                    description: format!("System {name}"),
                    short_description: None,
                    root: PathBuf::from("/test/system"),
                    path: PathBuf::from(format!("/test/system/{name}/SKILL.md")),
                    scope: SkillScope::System,
                    source_plugin: None,
                    source_install: None,
                    enabled: true,
                    read_only: true,
                    metadata: None,
                })
                .collect(),
            errors: vec![],
        })
    }
}
