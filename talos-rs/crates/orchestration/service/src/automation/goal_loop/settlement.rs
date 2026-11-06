use super::*;

const DEFAULT_GOAL_MAX_RUNTIME_SECONDS: i64 = 24 * 60 * 60;

impl SessionAutomationService {
    pub(crate) fn goal_budget_exhausted(goal: &SessionGoal, now: i64) -> bool {
        goal.token_budget
            .is_some_and(|budget| goal.tokens_used >= budget)
            || now
                >= goal
                    .created_at
                    .saturating_add(DEFAULT_GOAL_MAX_RUNTIME_SECONDS)
    }

    pub(crate) async fn settle(
        &self,
        session: &SessionRecord,
        event: &AgentEvent,
        metrics: Option<&AgentRunMetrics>,
    ) -> TalosResult<()> {
        let _gate = self.lock(&session.id).await;
        let _changed = SettlementChanged {
            sender: self.changed.clone(),
            session: session.id.clone(),
        };
        let now = chrono::Utc::now().timestamp();
        let observed = if metrics.is_none() {
            self.store.observed_metrics(&event.identity).await?
        } else {
            None
        };
        if let Some(metrics) = metrics.or(observed.as_ref()) {
            self.store
                .account_usage(&event.identity, metrics.usage.total_tokens.into(), now)
                .await?;
        }
        let origin = self.store.origin(&event.identity).await?;
        let declaration = self
            .declarations
            .lock()
            .expect("automation declarations")
            .remove(&turn_key(&event.identity));
        let waiting = matches!(event.payload, AgentEventPayload::WaitingForInput(_));
        let failed = match &event.payload {
            AgentEventPayload::TurnFailed(f) => Some(f.error.code),
            _ => None,
        };
        let normal = matches!(event.payload, AgentEventPayload::TurnCompleted(_));
        if !waiting && !normal && failed.is_none() {
            return Ok(());
        }
        if normal && origin.is_none() {
            if let Some(input) = self.store.initial_user_input(&event.identity).await? {
                if let Some(mode) = explicit_automation_mode(&input)? {
                    if mode == AutomationMode::Loop
                        && self.store.has_loop_activation(&event.identity).await?
                    {
                        return Ok(());
                    }
                    // The execution outcome is already committed. The original
                    // completion owner publishes this missing product effect as
                    // a notice without rewriting the Turn or creating a task.
                    return Err(TalosError::new(
                        ErrorCode::ToolExecutionFailed,
                        match mode {
                            AutomationMode::Goal => "No Goal was created in this Turn.",
                            AutomationMode::Loop => "No Loop was created in this Turn.",
                        },
                    ));
                }
            }
        }
        if let Some(origin) = origin.as_ref() {
            self.forget_explicit_dispatch(&session.id, &automation_object(origin));
        }
        if let Some(goal) = self.store.snapshot(&session.id).await?.goal {
            let current_work = matches!(
                origin.as_ref(),
                Some(AutomationTurnOrigin::Goal { goal_id, objective_revision })
                    if goal_id == &goal.id
                        && *objective_revision == goal.objective_revision
                        && goal.latest_turn_id.as_deref().is_none_or(|turn_id| turn_id == event.identity.turn_id)
            );
            if current_work && goal.status == GoalStatus::Active && (waiting || failed.is_some()) {
                self.pause(
                    &goal,
                    if waiting {
                        GoalPauseReason::WaitingForUser
                    } else if failed == Some(ErrorCode::Cancelled) {
                        GoalPauseReason::UserCancelled
                    } else {
                        GoalPauseReason::ExecutionFailed
                    },
                    now,
                )
                .await?;
            }
        }
        match origin {
            Some(AutomationTurnOrigin::Goal {
                goal_id,
                objective_revision,
            }) if normal => {
                let goal = self.store.goal(&goal_id).await?;
                if goal.status != GoalStatus::Active
                    || goal.objective_revision != objective_revision
                {
                    return Ok(());
                }
                // Registration is bound to the Input but has admitted no work yet.
                let registered = goal.latest_turn_id.is_none();
                if let Some(Declaration::Goal {
                    goal_id: id,
                    revision,
                    evidence,
                }) = declaration
                {
                    if id == goal.id && revision == goal.objective_revision {
                        let mut next = goal.clone();
                        next.status = GoalStatus::Complete;
                        next.completion_evidence = Some(evidence);
                        next.updated_at = now;
                        self.store.replace_goal(&goal, &next).await?;
                        return Ok(());
                    }
                }
                if Self::goal_budget_exhausted(&goal, now) {
                    self.pause(&goal, GoalPauseReason::BudgetExhausted, now)
                        .await?;
                } else if let Some(metrics) = metrics {
                    if !registered
                        && (metrics.tool_calls == 0 || !self.goal_has_work_tool(event).await?)
                    {
                        self.pause(&goal, GoalPauseReason::NoProgress, now).await?;
                    }
                }
            }
            Some(AutomationTurnOrigin::Loop {
                loop_id,
                config_revision,
                ..
            }) => {
                let task = match self.store.loop_task(&loop_id).await {
                    Ok(task) => task,
                    Err(e) if e.is_code(ErrorCode::ResourceNotFound) => return Ok(()),
                    Err(e) => return Err(e),
                };
                if task.status != LoopStatus::Active
                    || task.latest_turn_id.as_deref() != Some(event.identity.turn_id.as_str())
                {
                    return Ok(());
                }
                if waiting || failed.is_some() {
                    self.stop(
                        &task,
                        if waiting {
                            LoopStopReason::WaitingForUser
                        } else if failed == Some(ErrorCode::Cancelled) {
                            LoopStopReason::UserCancelled
                        } else {
                            LoopStopReason::ExecutionFailed
                        },
                        None,
                        now,
                    )
                    .await?;
                    return Ok(());
                }
                if normal {
                    // A duplicate terminal callback cannot replace a known schedule
                    // or recreate an occurrence already waiting in the original queue.
                    if task.next_due_at.is_some() || self.queue.list(&session.id)?.messages.iter()
                        .any(|message| matches!(&message.source, QueuedMessageSource::Loop { loop_id, .. } if loop_id == &task.id))
                    {
                        return Ok(());
                    }
                    let mut next = task.clone();
                    next.updated_at = now;
                    match task.schedule {
                        LoopSchedule::FixedInterval { minutes } => {
                            next.next_due_at = Some(next_due(event.occurred_at, minutes)?);
                            self.store.replace_loop(&task, &next).await?;
                        }
                        LoopSchedule::ModelDecided {}
                            if config_revision != task.config_revision =>
                        {
                            if task.next_due_at.is_none() {
                                self.enqueue_loop(session, &task).await?;
                            }
                        }
                        LoopSchedule::ModelDecided {} => match declaration {
                            Some(Declaration::Loop {
                                loop_id: id,
                                revision,
                                decision,
                            }) if id == task.id && revision == config_revision => match decision {
                                LoopWakeDecision::Continue {
                                    delay_minutes,
                                    reason,
                                } => {
                                    next.next_due_at =
                                        Some(next_due(event.occurred_at, delay_minutes)?);
                                    next.reason = Some(reason);
                                    self.store.replace_loop(&task, &next).await?;
                                }
                                LoopWakeDecision::Stop { reason } => {
                                    self.stop(
                                        &task,
                                        LoopStopReason::ModelStopped,
                                        Some(reason),
                                        now,
                                    )
                                    .await?
                                }
                            },
                            _ if metrics.is_some() => {
                                self.stop(&task, LoopStopReason::MissingDecision, None, now)
                                    .await?
                            }
                            _ => {} // Recovered records do not prove the live model omitted a decision.
                        },
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    /// Inspect existing committed facts only. Control reads/decisions are not
    /// business progress; ordinary Tool calls retain the original progress policy.
    async fn goal_has_work_tool(&self, terminal: &AgentEvent) -> TalosResult<bool> {
        let through = self
            .execution
            .get_latest_agent_event_seq(&terminal.identity.session_id)
            .await?
            .unwrap_or_default();
        let mut progressed = false;
        crate::timeline::scan_turn_events(
            self.execution.as_ref(),
            &terminal.identity,
            through,
            |record| {
                if record.sequence > terminal.sequence {
                    return Ok(());
                }
                let work_tool = match record.payload {
                    AgentEventPayload::MessageBlockCompleted(completed) => {
                        is_business_tool_call(&completed.block)
                    }
                    AgentEventPayload::MessageCompleted(message)
                        if message.role == AgentMessageRole::Agent =>
                    {
                        message.blocks.iter().any(is_business_tool_call)
                    }
                    _ => false,
                };
                progressed |= work_tool;
                Ok(())
            },
        )
        .await?;
        Ok(progressed)
    }

    pub(crate) async fn scan(&self, session: &SessionRecord, now: i64) -> TalosResult<()> {
        let _gate = self.lock(&session.id).await;
        for task in self
            .store
            .loops(&session.id)
            .await?
            .into_iter()
            .filter(|l| l.status == LoopStatus::Active)
        {
            if task.next_due_at.is_some_and(|due| due <= now) {
                self.enqueue_loop(session, &task).await?;
            }
        }
        Ok(())
    }
}

fn is_business_tool_call(block: &talos_agent_api::AgentMessageBlock) -> bool {
    matches!(&block.content, AgentMessageBlockContent::ToolCall(call) if call.name != "automation")
}

struct SettlementChanged {
    sender: tokio::sync::broadcast::Sender<AutomationChangedNotification>,
    session: String,
}
impl Drop for SettlementChanged {
    fn drop(&mut self) {
        let _ = self.sender.send(AutomationChangedNotification {
            session_id: self.session.clone(),
        });
    }
}
