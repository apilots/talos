use super::*;

impl TurnService {
    pub async fn submit_dynamic_tool_result(
        &self,
        session: TalosSession,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        let session_id = session.context.session_id.clone();
        let active = self
            .event_service
            .get_active_turn(&session_id)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::Conflict,
                    "No pending user action for this session.",
                )
            })?;
        let action_id = active
            .current_action_id
            .clone()
            .ok_or_else(|| TalosError::internal("active turn has no current user action"))?;
        let action = self
            .user_actions
            .get(&session_id, &active.turn_id, &action_id)
            .await?
            .ok_or_else(|| {
                TalosError::internal(format!(
                    "active turn references missing user action {action_id}"
                ))
            })?;
        let UserActionParams::DynamicToolResult(params) = &action.action.params else {
            return Err(TalosError::public(
                talos_error::ErrorCode::Conflict,
                "The pending user action is not a dynamic tool result.",
            ));
        };
        if action.action.call_id != call_id || params.tool_name != tool_name {
            return Err(TalosError::public(
                talos_error::ErrorCode::Conflict,
                "Pending dynamic tool result identity does not match.",
            ));
        }
        self.submit_user_action(
            session,
            &active.turn_id,
            &action_id,
            UserActionResponse::DynamicToolResult(talos_types::DynamicToolResult {
                output,
                success,
            }),
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn submit_user_action(
        &self,
        session: TalosSession,
        turn_id: &str,
        action_id: &str,
        response: UserActionResponse,
    ) -> TalosResult<UserActionState> {
        let session_id = session.context.session_id.clone();
        let current = self
            .user_actions
            .get(&session_id, turn_id, action_id)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("user action {action_id}"),
                )
            })?;
        let pending_resume = if current.action.state == UserActionState::Pending {
            Some(
                self.pending_user_action_resume_context(&session_id, turn_id, action_id)
                    .await?,
            )
        } else {
            None
        };
        let accepted = self
            .user_actions
            .accept(&session_id, turn_id, action_id, response)
            .await?;
        if !accepted.should_resume {
            return Ok(accepted.record.action.state);
        }
        self.resume_accepted_user_action(
            session,
            turn_id,
            action_id,
            accepted.record,
            pending_resume,
        )
        .await
    }

    async fn resume_accepted_user_action(
        &self,
        session: TalosSession,
        turn_id: &str,
        action_id: &str,
        accepted: talos_store::session::UserActionRecord,
        pending_resume: Option<ActiveResumeContext>,
    ) -> TalosResult<UserActionState> {
        let session_id = session.context.session_id.clone();
        if matches!(
            accepted.response,
            Some(UserActionResponse::SandboxApproval(
                SandboxApprovalDecision::Cancel
            ))
        ) {
            let active = self
                .event_service
                .get_active_turn(&session_id)
                .await?
                .ok_or_else(|| {
                    TalosError::public(
                        talos_error::ErrorCode::Conflict,
                        "cancelled user action is missing its active turn",
                    )
                })?;
            let record = self.user_actions.cancel(accepted).await?;
            self.event_service
                .finish_terminal_user_action(active, record)
                .await?;
            return Ok(UserActionState::Cancelled);
        }
        let resume = match pending_resume {
            Some(resume) => resume,
            None => {
                self.pending_user_action_resume_context(&session_id, turn_id, action_id)
                    .await?
            }
        };
        let tool_name = tool_name_for_action(&accepted.action.params);
        let response = accepted
            .response
            .clone()
            .ok_or_else(|| TalosError::internal("accepted user action is missing its response"))?;
        let binding = self.session_binding(&session_id).await?;
        let mut record = accepted;
        let mut response_persisted = false;

        for attempt in 0..2 {
            record = self.user_actions.begin_resume(record).await?;
            if !response_persisted {
                let dispatch = self
                    .executor
                    .submit_input(AgentDispatchInputParams {
                        binding: binding.clone(),
                        session: session.clone(),
                        turn_id: turn_id.to_string(),
                        input: AgentDispatchInput::UserActionResponse {
                            call_id: record.action.call_id.clone(),
                            tool_name: tool_name.clone(),
                            response: response.clone(),
                        },
                    })
                    .await;
                if let Err(error) = dispatch {
                    record = self.user_actions.retry_or_fail(record).await?;
                    if record.action.state == UserActionState::ResponseAccepted && attempt == 0 {
                        continue;
                    }
                    self.finish_failed_user_action(record).await?;
                    return Err(error);
                }
                response_persisted = true;
            }
            match self
                .resume_after_user_action(session.clone(), resume.clone(), binding.clone())
                .await
            {
                Ok(()) => return Ok(record.action.state),
                Err(error) => {
                    record = self.user_actions.retry_or_fail(record).await?;
                    if record.action.state != UserActionState::ResponseAccepted || attempt > 0 {
                        self.finish_failed_user_action(record).await?;
                        return Err(error);
                    }
                }
            }
        }
        Err(TalosError::internal(
            "user action resume attempts were exhausted",
        ))
    }

    pub(crate) async fn recover_user_action(
        &self,
        session: TalosSession,
        active: ActiveTurnRecord,
    ) -> TalosResult<()> {
        if active.status != ActiveTurnStatus::WaitingUserAction {
            return Ok(());
        }
        let Some(action_id) = active.current_action_id.as_deref() else {
            return Err(TalosError::internal(
                "waiting active turn is missing its user action id",
            ));
        };
        let record = self
            .user_actions
            .get(&active.session_id, &active.turn_id, action_id)
            .await?
            .ok_or_else(|| {
                TalosError::internal(format!(
                    "waiting active turn references missing user action {action_id}"
                ))
            })?;
        let recovered = self.user_actions.recover(record).await?;
        if recovered.should_resume {
            let resume = self
                .pending_user_action_resume_context(&active.session_id, &active.turn_id, action_id)
                .await?;
            return self
                .resume_accepted_user_action(
                    session,
                    &active.turn_id,
                    action_id,
                    recovered.record,
                    Some(resume),
                )
                .await
                .map(|_| ());
        }
        if matches!(
            recovered.record.action.state,
            UserActionState::ExecutionUnknown | UserActionState::Cancelled
        ) {
            self.event_service
                .finish_terminal_user_action(active, recovered.record)
                .await?;
        }
        Ok(())
    }

    async fn finish_failed_user_action(
        &self,
        record: talos_store::session::UserActionRecord,
    ) -> TalosResult<()> {
        if record.action.state != UserActionState::ResumeFailed {
            return Ok(());
        }
        let active = self
            .event_service
            .get_active_turn(&record.session_id)
            .await?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::Conflict,
                    "failed user action is missing its active turn",
                )
            })?;
        self.event_service
            .finish_terminal_user_action(active, record)
            .await
    }

    async fn pending_user_action_resume_context(
        &self,
        session_id: &str,
        turn_id: &str,
        action_id: &str,
    ) -> TalosResult<ActiveResumeContext> {
        let active = self
            .event_service
            .get_active_turn(session_id)
            .await
            .map_err(|err| TalosError::internal(format!("active turn lookup failed: {err}")))?
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::Conflict,
                    "No pending user action for this session.",
                )
            })?;
        if active.status != ActiveTurnStatus::WaitingUserAction {
            return Err(TalosError::public(
                talos_error::ErrorCode::Conflict,
                "The active turn is not waiting for user action.",
            ));
        }
        if active.turn_id != turn_id || active.current_action_id.as_deref() != Some(action_id) {
            return Err(TalosError::public(
                talos_error::ErrorCode::Conflict,
                "The pending user action identity does not match the active turn.",
            ));
        }
        let message_id = active.message_id.clone().ok_or_else(|| {
            TalosError::internal("pending user action is missing its original message id")
        })?;
        let Some(pending) = self.event_service.get_current_user_action(&active).await? else {
            return Err(TalosError::public(
                talos_error::ErrorCode::Conflict,
                "The active turn has no current user action.",
            ));
        };
        let item_id = pending.item_id.ok_or_else(|| {
            TalosError::internal("pending user action is missing its original timeline item id")
        })?;
        let events = self
            .event_service
            .list_turn_events(session_id, turn_id, 10_000)
            .await?;
        let tool_calls = nonterminal_resume_tool_calls(&events);
        let execution_metrics = latest_turn_execution_metrics(&events).ok_or_else(|| {
            TalosError::internal("pending user action is missing cumulative turn execution metrics")
        })?;
        if !tool_calls
            .iter()
            .any(|tool_call| tool_call.item_id == item_id && tool_call.call_id == pending.call_id)
        {
            return Err(TalosError::internal(
                "pending user action timeline item is not active",
            ));
        }
        Ok(ActiveResumeContext {
            turn_id: active.turn_id,
            message_id,
            gateway_resume: Some(AgentDispatchResumeContext {
                pending_call_id: pending.call_id,
                tool_calls,
                execution_metrics,
            }),
        })
    }
}

pub(crate) fn nonterminal_resume_tool_calls(
    events: &[AgentEvent],
) -> Vec<AgentDispatchResumeToolCall> {
    let mut order = Vec::new();
    let mut calls =
        HashMap::<String, (AgentDispatchResumeToolCall, TalosTimelineItemStatus)>::new();
    for event in events {
        match &event.payload {
            AgentEventPayload::ItemStarted(started) => {
                let TalosTimelineContent::ToolCall { call_id, name, .. } = &started.item.content
                else {
                    continue;
                };
                order.push(started.item.item_id.clone());
                calls.insert(
                    started.item.item_id.clone(),
                    (
                        AgentDispatchResumeToolCall {
                            item_id: started.item.item_id.clone(),
                            call_id: call_id.clone(),
                            tool_name: name.clone(),
                        },
                        started.item.status,
                    ),
                );
            }
            AgentEventPayload::ItemDelta(delta) => {
                if let TalosTimelineDelta::Status { status } = &delta.delta {
                    if let Some((_, current)) = calls.get_mut(&delta.item_id) {
                        *current = *status;
                    }
                }
            }
            AgentEventPayload::ItemCompleted(completed) => {
                if let Some((_, current)) = calls.get_mut(&completed.item_id) {
                    *current = completed.status;
                }
            }
            AgentEventPayload::TurnStarted(_)
            | AgentEventPayload::AssistantTextCommitted(_)
            | AgentEventPayload::TurnMetrics(_)
            | AgentEventPayload::TurnCompleted(_)
            | AgentEventPayload::TurnError(_) => {}
        }
    }
    order
        .into_iter()
        .filter_map(|item_id| calls.remove(&item_id))
        .filter_map(|(call, status)| {
            matches!(
                status,
                TalosTimelineItemStatus::Queued
                    | TalosTimelineItemStatus::Running
                    | TalosTimelineItemStatus::WaitingUserAction
            )
            .then_some(call)
        })
        .collect()
}

fn latest_turn_execution_metrics(
    events: &[AgentEvent],
) -> Option<talos_types::talos::TurnExecutionMetrics> {
    events.iter().rev().find_map(|event| {
        let AgentEventPayload::TurnCompleted(completed) = &event.payload else {
            return None;
        };
        completed.metrics.clone()
    })
}

fn tool_name_for_action(params: &UserActionParams) -> String {
    match params {
        UserActionParams::DynamicToolResult(params) => params.tool_name.clone(),
        UserActionParams::Question(_) => "AskUserQuestion".to_string(),
        UserActionParams::SandboxApproval(params) => params.tool_name.clone(),
    }
}
