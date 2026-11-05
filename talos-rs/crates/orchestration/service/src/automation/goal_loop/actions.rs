use super::*;
#[path = "action_authorization.rs"]
mod authorization;
impl SessionAutomationService {
    pub(crate) async fn execute(
        &self,
        session: &SessionRecord,
        action: AutomationAction,
        source: AutomationCaller<'_>,
    ) -> TalosResult<AutomationControlResult> {
        let (caller, call_id) = match source {
            AutomationCaller::User => (None, None),
            AutomationCaller::Model { caller, call_id } => (Some(caller), Some(call_id)),
        };
        if !matches!(
            session.tree_identity,
            talos_store::session::SessionTreeIdentity::Primary
        ) {
            return Err(invalid(
                "Session automation is supported only on primary Sessions.",
            ));
        }
        let _gate = self.lock(&session.id).await;
        if call_id.is_some_and(|id| id.trim().is_empty()) {
            return Err(invalid("A trusted Tool call identity is required."));
        }
        if let Some(caller) = caller {
            if caller.session_id != session.id || caller.agent_id != session.agent_id {
                return Err(conflict());
            }
            let active = self
                .execution
                .get_agent_active_turn(&session.id)
                .await?
                .ok_or_else(conflict)?;
            if active.identity != caller.turn_ref() || active.state != ActiveTurnState::Running {
                return Err(conflict());
            }
        }
        if let Some(caller) = caller {
            self.validate_bound_action(caller, &action).await?;
        }
        let read_goal = matches!(
            &action,
            AutomationAction::Goal {
                action: GoalAction::Read {}
            }
        );
        let notify = !matches!(
            &action,
            AutomationAction::Goal {
                action: GoalAction::Read {} | GoalAction::Complete { .. }
            } | AutomationAction::Loop {
                action: LoopAction::List {} | LoopAction::Decide { .. }
            }
        );
        let now = chrono::Utc::now().timestamp();
        let mut effect = AutomationEffect::Applied;
        let mut created_object = None;
        match action {
            AutomationAction::Goal { action } => match action {
                GoalAction::Create {
                    objective,
                    token_budget,
                } => {
                    if objective.trim().is_empty()
                        || token_budget == Some(0)
                        || token_budget.is_some_and(|v| v > i64::MAX as u64)
                    {
                        return Err(invalid(
                            "A Goal requires an objective and a positive optional token budget.",
                        ));
                    }
                    if let Some(caller) = caller {
                        if let Some(AutomationTurnOrigin::Goal {
                            goal_id,
                            objective_revision,
                        }) = self.store.origin(&caller.turn_ref()).await?
                        {
                            return Ok(AutomationControlResult {
                                bound_origin: Some(AutomationTurnOrigin::Goal {
                                    goal_id: goal_id.clone(),
                                    objective_revision,
                                }),
                                created_object: Some(AutomationObjectRef::Goal { goal_id }),
                                effect: AutomationEffect::Applied,
                                snapshot: self.store.snapshot(&session.id).await?,
                            });
                        }
                    }
                    self.require_creation(session, AutomationMode::Goal, caller)
                        .await?;
                    let goal = SessionGoal {
                        id: format!("goal_{}", uuid::Uuid::now_v7().simple()),
                        session_id: session.id.clone(),
                        objective,
                        objective_revision: 1,
                        status: GoalStatus::Active,
                        pause_reason: None,
                        reason: None,
                        token_budget,
                        tokens_used: 0,
                        latest_turn_id: None,
                        completion_evidence: None,
                        created_at: now,
                        updated_at: now,
                    };
                    self.store
                        .create_goal(&goal, caller.map(AgentCaller::turn_ref).as_ref())
                        .await?;
                    created_object = Some(AutomationObjectRef::Goal {
                        goal_id: goal.id.clone(),
                    });
                }
                GoalAction::Read {} => {}
                GoalAction::Complete {
                    goal_id,
                    objective_revision,
                    evidence,
                } => {
                    let caller = caller.ok_or_else(|| {
                        invalid("Goal completion requires the current model invocation.")
                    })?;
                    if evidence.trim().is_empty() {
                        return Err(invalid("Goal completion requires evidence."));
                    }
                    let goal = self.owned_goal(&session.id, &goal_id).await?;
                    if goal.status != GoalStatus::Active
                        || goal.latest_turn_id.as_deref() != Some(caller.turn_id.as_str())
                        || goal.objective_revision != objective_revision
                        || self.store.origin(&caller.turn_ref()).await?
                            != Some(AutomationTurnOrigin::Goal {
                                goal_id: goal_id.clone(),
                                objective_revision,
                            })
                    {
                        return Err(conflict());
                    }
                    self.declarations
                        .lock()
                        .expect("automation declarations")
                        .insert(
                            turn_key(&caller.turn_ref()),
                            Declaration::Goal {
                                goal_id,
                                revision: objective_revision,
                                evidence,
                            },
                        );
                    effect = AutomationEffect::PendingSettlement;
                }
                GoalAction::Edit {
                    goal_id,
                    expected_revision,
                    objective,
                } => {
                    let goal = self.owned_goal(&session.id, &goal_id).await?;
                    if goal.objective_revision != expected_revision {
                        return Err(changed_revision());
                    }
                    if !matches!(goal.status, GoalStatus::Active | GoalStatus::Paused)
                        || objective.trim().is_empty()
                    {
                        return Err(invalid(
                            "Only unfinished Goals with a nonempty objective may be edited.",
                        ));
                    }
                    let mut next = goal.clone();
                    if next.objective != objective {
                        next.objective = objective;
                        next.objective_revision = next_revision(next.objective_revision)?;
                    }
                    next.updated_at = now;
                    self.store.replace_goal(&goal, &next).await?;
                }
                GoalAction::Pause { goal_id } => {
                    let goal = self.owned_goal(&session.id, &goal_id).await?;
                    if goal.status != GoalStatus::Active {
                        return Err(conflict());
                    }
                    let automatic = if let Some(caller) = caller {
                        self.store.origin(&caller.turn_ref()).await?.is_some()
                            && !self.store.has_user_steer(&caller.turn_ref()).await?
                    } else {
                        false
                    };
                    self.pause(
                        &goal,
                        if automatic {
                            GoalPauseReason::NoProgress
                        } else {
                            GoalPauseReason::UserPaused
                        },
                        now,
                    )
                    .await?;
                }
                GoalAction::Resume { goal_id } => {
                    let goal = self.owned_goal(&session.id, &goal_id).await?;
                    if goal.status != GoalStatus::Paused {
                        return Err(conflict());
                    }
                    let snapshot = self.store.snapshot(&session.id).await?;
                    if !snapshot.loops.is_empty() {
                        return Err(conflict());
                    }
                    for message in self.queue.list(&session.id)?.messages {
                        if explicit_automation_mode(&message.input)? == Some(AutomationMode::Loop) {
                            return Err(conflict());
                        }
                    }
                    if let Some(active) = self.execution.get_agent_active_turn(&session.id).await? {
                        if active.state != ActiveTurnState::Running
                            || explicit_automation_mode(
                                &self.initial_input(&active.identity).await?,
                            )? == Some(AutomationMode::Loop)
                        {
                            return Err(conflict());
                        }
                    }
                    let mut next = goal.clone();
                    next.status = GoalStatus::Active;
                    next.pause_reason = None;
                    next.reason = None;
                    next.updated_at = now;
                    self.store.replace_goal(&goal, &next).await?;
                }
                GoalAction::Clear { goal_id } => {
                    let goal = self.owned_goal(&session.id, &goal_id).await?;
                    let mut next = goal.clone();
                    next.status = GoalStatus::Cleared;
                    next.updated_at = now;
                    self.store.replace_goal(&goal, &next).await?;
                    self.forget_explicit_dispatch(
                        &session.id,
                        &AutomationObjectRef::Goal { goal_id },
                    );
                }
            },
            AutomationAction::Loop { action } => match action {
                LoopAction::Create { prompt, schedule } => {
                    schedule.validate()?;
                    if prompt.trim().is_empty() {
                        return Err(invalid("A Loop requires a prompt."));
                    }
                    self.require_creation(session, AutomationMode::Loop, caller)
                        .await?;
                    let task = SessionLoop {
                        id: format!("loop_{}", uuid::Uuid::now_v7().simple()),
                        session_id: session.id.clone(),
                        prompt,
                        schedule,
                        config_revision: 1,
                        status: LoopStatus::Active,
                        stop_reason: None,
                        reason: None,
                        next_due_at: Some(now.saturating_add(AUTOMATION_INITIAL_DELAY_SECONDS)),
                        latest_turn_id: None,
                        created_at: now,
                        updated_at: now,
                    };
                    // Validate the actual System Skill now; the existing scan prepares
                    // the occurrence only after the initial cancellation window.
                    self.skill_input(session, AutomationMode::Loop, task.prompt.clone())
                        .await?;
                    if let Some(caller) = caller {
                        let created = self
                            .store
                            .create_loop_for_call(
                                &task,
                                &caller.turn_ref(),
                                call_id.ok_or_else(conflict)?,
                            )
                            .await?;
                        created_object = Some(AutomationObjectRef::Loop {
                            loop_id: created.id.clone(),
                        });
                    } else {
                        self.store.create_loop(&task).await?;
                        created_object = Some(AutomationObjectRef::Loop {
                            loop_id: task.id.clone(),
                        });
                    }
                }
                LoopAction::List {} => {}
                LoopAction::Decide {
                    loop_id,
                    config_revision,
                    decision,
                } => {
                    let caller = caller.ok_or_else(|| {
                        invalid("Loop decisions require the current model invocation.")
                    })?;
                    let task = self.owned_loop(&session.id, &loop_id).await?;
                    if task.status != LoopStatus::Active
                        || task.config_revision != config_revision
                        || !matches!(task.schedule, LoopSchedule::ModelDecided {})
                    {
                        return Err(conflict());
                    }
                    if let LoopWakeDecision::Continue {
                        delay_minutes: 0, ..
                    } = decision
                    {
                        return Err(invalid("Loop interval must be at least one minute."));
                    }
                    if !matches!(self.store.origin(&caller.turn_ref()).await?,Some(AutomationTurnOrigin::Loop{loop_id:ref id,config_revision:rev,..}) if id==&loop_id && rev==config_revision)
                    {
                        return Err(conflict());
                    }
                    self.declarations
                        .lock()
                        .expect("automation declarations")
                        .insert(
                            turn_key(&caller.turn_ref()),
                            Declaration::Loop {
                                loop_id,
                                revision: config_revision,
                                decision,
                            },
                        );
                    effect = AutomationEffect::PendingSettlement;
                }
                LoopAction::Edit {
                    loop_id,
                    expected_revision,
                    prompt,
                    schedule,
                } => {
                    schedule.validate()?;
                    let task = self.owned_loop(&session.id, &loop_id).await?;
                    if task.config_revision != expected_revision {
                        return Err(changed_revision());
                    }
                    if task.status != LoopStatus::Active || prompt.trim().is_empty() {
                        return Err(conflict());
                    }
                    let mut next = task.clone();
                    let schedule_changed = task.schedule != schedule;
                    if task.prompt != prompt || schedule_changed {
                        next.config_revision = next_revision(task.config_revision)?;
                    }
                    next.prompt = prompt;
                    next.schedule = schedule;
                    next.updated_at = now;
                    let queued=self.queue.list(&session.id)?.messages.iter().any(|m|matches!(&m.source,QueuedMessageSource::Loop{loop_id,..} if loop_id==&task.id));
                    let running = if let Some(active) =
                        self.execution.get_agent_active_turn(&session.id).await?
                    {
                        matches!(self.store.origin(&active.identity).await?,Some(AutomationTurnOrigin::Loop{loop_id,..}) if loop_id==task.id)
                    } else {
                        false
                    };
                    let initial_wait = task.latest_turn_id.is_none()
                        && task.next_due_at
                            == Some(
                                task.created_at
                                    .saturating_add(AUTOMATION_INITIAL_DELAY_SECONDS),
                            );
                    if let LoopSchedule::FixedInterval { minutes } = next.schedule {
                        if !initial_wait && (schedule_changed || next.next_due_at.is_none()) {
                            next.next_due_at = if queued || running {
                                None
                            } else {
                                Some(next_due(now, minutes)?)
                            };
                        }
                    }
                    self.store.replace_loop(&task, &next).await?;
                    if next.config_revision != task.config_revision
                        && matches!(next.schedule, LoopSchedule::ModelDecided {})
                        && next.next_due_at.is_none()
                        && !queued
                        && !running
                    {
                        self.enqueue_loop(session, &next).await?;
                    }
                }
                LoopAction::Stop { loop_id } => {
                    let task = self.owned_loop(&session.id, &loop_id).await?;
                    self.stop(&task, LoopStopReason::UserStopped, None, now)
                        .await?;
                }
                LoopAction::Clear { loop_id } => {
                    let task = self.owned_loop(&session.id, &loop_id).await?;
                    self.store.clear_loop(&task).await?;
                    self.queue.remove_loop(&session.id, &loop_id)?;
                    self.forget_explicit_dispatch(
                        &session.id,
                        &AutomationObjectRef::Loop { loop_id },
                    );
                }
            },
        }
        if notify {
            self.emit(&session.id);
        }
        let bound_origin = if let Some(caller) = caller {
            self.store.origin(&caller.turn_ref()).await?
        } else {
            None
        };
        let mut snapshot = self.store.snapshot(&session.id).await?;
        if read_goal {
            if let Some(AutomationTurnOrigin::Goal { goal_id, .. }) = &bound_origin {
                // A bound Turn retains its original authority after the definition is cleared.
                // General management snapshots still omit cleared Goals.
                snapshot.goal = Some(self.owned_goal(&session.id, goal_id).await?);
            }
        }
        Ok(AutomationControlResult {
            created_object,
            bound_origin,
            effect,
            snapshot,
        })
    }
    async fn owned_goal(&self, session: &str, id: &str) -> TalosResult<SessionGoal> {
        let value = self.store.goal(id).await?;
        if value.session_id != session {
            return Err(TalosError::new(
                ErrorCode::ResourceNotFound,
                "Automation object was not found.",
            ));
        }
        Ok(value)
    }
    async fn owned_loop(&self, session: &str, id: &str) -> TalosResult<SessionLoop> {
        let value = self.store.loop_task(id).await?;
        if value.session_id != session {
            return Err(TalosError::new(
                ErrorCode::ResourceNotFound,
                "Automation object was not found.",
            ));
        }
        Ok(value)
    }
    pub(crate) async fn pause(
        &self,
        goal: &SessionGoal,
        reason: GoalPauseReason,
        now: i64,
    ) -> TalosResult<()> {
        let mut next = goal.clone();
        next.status = GoalStatus::Paused;
        next.pause_reason = Some(reason);
        next.updated_at = now;
        self.store.replace_goal(goal, &next).await?;
        self.forget_explicit_dispatch(
            &goal.session_id,
            &AutomationObjectRef::Goal {
                goal_id: goal.id.clone(),
            },
        );
        self.emit(&goal.session_id);
        Ok(())
    }
    pub(crate) async fn stop(
        &self,
        task: &SessionLoop,
        reason: LoopStopReason,
        detail: Option<String>,
        now: i64,
    ) -> TalosResult<()> {
        self.forget_explicit_dispatch(
            &task.session_id,
            &AutomationObjectRef::Loop {
                loop_id: task.id.clone(),
            },
        );
        if task.status != LoopStatus::Active {
            return Ok(());
        }
        let mut next = task.clone();
        next.status = LoopStatus::Stopped;
        next.stop_reason = Some(reason);
        next.reason = detail;
        next.next_due_at = None;
        next.updated_at = now;
        self.store.replace_loop(task, &next).await?;
        self.queue.remove_loop(&task.session_id, &task.id)?;
        self.emit(&task.session_id);
        Ok(())
    }
}
