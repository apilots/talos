use super::*;
use talos_agent_api::{
    ActiveTurnState, AgentEventPayload as CanonicalAgentEventPayload, AgentInterruptOutcome,
    AgentMessageBlockContent, AgentMessageRole, AgentPendingInput, AgentUserContent,
};
use talos_store::session::{
    AgentActiveTurnRecord, AgentContinuationBindingStatus, AgentEventRecord,
};
use talos_types::{QuestionAnswer, UserActionParams};

impl TurnService {
    pub(crate) async fn submit_user_action(
        &self,
        session: talos_store::session::SessionRecord,
        turn_id: &str,
        action_id: &str,
        response: UserActionResponse,
        command: Option<talos_store::session::CommandLease>,
    ) -> TalosResult<UserActionState> {
        let identity = AgentTurnRef::new(&session.id, turn_id);
        let events = self
            .canonical_user_action_events(&identity, action_id)
            .await?;
        let action = requested_user_action(&events, Some(action_id)).ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("user action {action_id}"),
            )
        })?;
        validate_user_action_response(&action.params, &response)?;

        if let Some(existing) = submitted_user_action_response(&events, action_id) {
            if existing != &response {
                return Err(TalosError::public(
                    talos_error::ErrorCode::Conflict,
                    format!("user action `{action_id}` already has a different response"),
                ));
            }
            self.release_command(command).await;
            return self.user_action_state(&identity, &events, &response).await;
        }

        self.canonical_active_turn(&identity.session_id)
            .await?
            .filter(|active| {
                active.identity == identity
                    && active.state == ActiveTurnState::WaitingForInput
                    && active.current_action_id.as_deref() == Some(action_id)
            })
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::Conflict,
                    "The pending user action identity does not match the active turn.",
                )
            })?;
        let context = self
            .sessions
            .build_agent_context(&session, identity.clone())
            .await?;
        let message = crate::turn::completed_user_action_message(
            turn_id,
            &format!("msg_action_{action_id}"),
            action_id,
            response.clone(),
            chrono::Utc::now().timestamp(),
        )?;
        let completion = match self
            .execution
            .resume_waiting_input(context, message, command)
            .await
        {
            Ok(completion) => completion,
            Err(error) => {
                let current = self
                    .canonical_user_action_events(&identity, action_id)
                    .await?;
                if submitted_user_action_response(&current, action_id) == Some(&response) {
                    return self.user_action_state(&identity, &current, &response).await;
                }
                return Err(error);
            }
        };
        self.complete_resumed_turn(session, completion);
        Ok(UserActionState::Resuming)
    }

    pub(crate) async fn recover_active_turn(
        &self,
        session: talos_store::session::SessionRecord,
        active: AgentActiveTurnRecord,
    ) -> TalosResult<()> {
        let service = self.clone();
        super::await_admission(
            async move { service.recover_active_turn_owned(session, active).await },
        )
        .await
    }

    async fn recover_active_turn_owned(
        &self,
        session: talos_store::session::SessionRecord,
        active: AgentActiveTurnRecord,
    ) -> TalosResult<()> {
        let binding = self
            .execution_store
            .get_agent_continuation_binding(&active.identity)
            .await?;
        if active.state == ActiveTurnState::WaitingForInput
            && binding.as_ref().is_some_and(|binding| {
                matches!(&binding.status, AgentContinuationBindingStatus::Waiting)
            })
        {
            return Ok(());
        }
        if active.state == ActiveTurnState::Cancelling {
            return self
                .terminate_recovered_turn(
                    &session,
                    active,
                    talos_error::ErrorCode::Cancelled,
                    "Agent Turn cancellation recovered after confirmed cleanup",
                )
                .await;
        }
        let Some(binding) = binding else {
            return self
                .terminate_recovered_turn(
                    &session,
                    active,
                    talos_error::ErrorCode::ServiceUnavailable,
                    "Agent Turn has no durable continuation after Service restart",
                )
                .await;
        };
        let AgentContinuationBindingStatus::Claimed { claim_id } = &binding.status else {
            return self
                .terminate_recovered_turn(
                    &session,
                    active,
                    talos_error::ErrorCode::ServiceUnavailable,
                    "Agent Turn continuation state cannot resume after Service restart",
                )
                .await;
        };
        let events = self
            .canonical_user_action_events(&active.identity, &binding.action_id)
            .await?;
        let message = events.iter().find_map(|event| match &event.payload {
            CanonicalAgentEventPayload::MessageCompleted(message)
                if message.message_id == claim_id.as_str()
                    && message.role == AgentMessageRole::User
                    && submitted_user_action_response(
                        std::slice::from_ref(event),
                        &binding.action_id,
                    )
                    .is_some() =>
            {
                Some((message.clone(), event.event_seq))
            }
            _ => None,
        });
        let Some((message, admission_tail)) = message else {
            return self
                .terminate_recovered_turn(
                    &session,
                    active,
                    talos_error::ErrorCode::ServiceUnavailable,
                    "Claimed Agent continuation is missing its canonical User response",
                )
                .await;
        };
        if active.last_event_seq != admission_tail {
            return self.terminate_recovered_turn(
                &session,
                active,
                talos_error::ErrorCode::ServiceUnavailable,
                "Claimed Agent continuation already produced progress and cannot be replayed safely",
            ).await;
        }
        let context = self
            .sessions
            .build_agent_context(&session, active.identity.clone())
            .await?;
        let resume = match self
            .execution
            .resume_waiting_input(context, message, None)
            .await
        {
            Ok(completion) => {
                self.complete_resumed_turn(session.clone(), completion);
                Ok(())
            }
            Err(error) => Err(error),
        };
        match resume {
            Ok(()) => Ok(()),
            Err(error) => {
                self.terminate_recovered_turn(
                    &session,
                    active,
                    error.error_code(),
                    &format!(
                        "Agent continuation could not resume after Service restart: {}",
                        error.diagnostic_message()
                    ),
                )
                .await
            }
        }
    }

    async fn terminate_recovered_turn(
        &self,
        session: &talos_store::session::SessionRecord,
        active: AgentActiveTurnRecord,
        code: talos_error::ErrorCode,
        message: &str,
    ) -> TalosResult<()> {
        let outcome = self
            .recover_terminal_turn(
                session,
                &active.identity,
                talos_agent_api::AgentErrorInfo {
                    code,
                    message: message.to_string(),
                    retryable: false,
                },
            )
            .await?;
        match outcome {
            AgentInterruptOutcome::Cancelled | AgentInterruptOutcome::Completed => Ok(()),
            AgentInterruptOutcome::CleanupUnconfirmed { task_count } => Err(TalosError::public(
                talos_error::ErrorCode::RunCleanupUnconfirmed,
                format!("Agent cleanup is unconfirmed for {task_count} task(s)"),
            )),
        }
    }

    async fn canonical_user_action_events(
        &self,
        identity: &AgentTurnRef,
        action_id: &str,
    ) -> TalosResult<Vec<AgentEventRecord>> {
        let tail = self
            .execution_store
            .get_latest_agent_event_seq(&identity.session_id)
            .await?
            .unwrap_or_default();
        let mut events = Vec::new();
        crate::timeline::scan_turn_events(self.execution_store.as_ref(), identity, tail, |event| {
            if matches!(
                &event.payload,
                CanonicalAgentEventPayload::TurnCompleted(_)
                    | CanonicalAgentEventPayload::TurnFailed(_)
            ) || matches!(&event.payload, CanonicalAgentEventPayload::WaitingForInput(pending)
                    if pending.action_id() == action_id)
                || submitted_user_action_response(std::slice::from_ref(&event), action_id).is_some()
            {
                events.push(event);
            }
            Ok(())
        })
        .await?;
        Ok(events)
    }

    async fn user_action_state(
        &self,
        identity: &AgentTurnRef,
        events: &[AgentEventRecord],
        response: &UserActionResponse,
    ) -> TalosResult<UserActionState> {
        if let Some(terminal) = events.iter().rev().find_map(|event| match &event.payload {
            CanonicalAgentEventPayload::TurnCompleted(_) => Some(UserActionState::Resolved),
            CanonicalAgentEventPayload::TurnFailed(_) => Some(
                if matches!(
                    response,
                    UserActionResponse::SandboxApproval(
                        talos_types::SandboxApprovalDecision::Cancel
                    )
                ) {
                    UserActionState::Cancelled
                } else {
                    UserActionState::ResumeFailed
                },
            ),
            _ => None,
        }) {
            return Ok(terminal);
        }
        let active = self.canonical_active_turn(&identity.session_id).await?;
        match active.filter(|active| active.identity == *identity) {
            Some(active) if active.state == ActiveTurnState::Running => {
                Ok(UserActionState::Resuming)
            }
            Some(active) if active.state == ActiveTurnState::Cancelling => {
                Ok(UserActionState::ExecutionUnknown)
            }
            Some(_) => Ok(UserActionState::ResponseAccepted),
            None => Err(TalosError::internal(
                "answered user action has neither active nor terminal Turn state",
            )),
        }
    }
}

fn requested_user_action<'a>(
    events: &'a [AgentEventRecord],
    action_id: Option<&str>,
) -> Option<&'a talos_types::PendingUserAction> {
    events.iter().rev().find_map(|event| match &event.payload {
        CanonicalAgentEventPayload::WaitingForInput(AgentPendingInput::UserAction { action })
            if action_id.is_none_or(|action_id| action.action_id == action_id) =>
        {
            Some(action)
        }
        _ => None,
    })
}

fn submitted_user_action_response<'a>(
    events: &'a [AgentEventRecord],
    action_id: &str,
) -> Option<&'a UserActionResponse> {
    events.iter().rev().find_map(|event| match &event.payload {
        CanonicalAgentEventPayload::MessageCompleted(message)
            if message.role == AgentMessageRole::User && message.blocks.len() == 1 =>
        {
            match &message.blocks[0].content {
                AgentMessageBlockContent::UserContent(AgentUserContent::UserActionResponse {
                    action_id: submitted_action_id,
                    response,
                }) if submitted_action_id == action_id => Some(response),
                _ => None,
            }
        }
        _ => None,
    })
}

fn validate_user_action_response(
    params: &UserActionParams,
    response: &UserActionResponse,
) -> TalosResult<()> {
    match (params, response) {
        (UserActionParams::DynamicToolResult(_), UserActionResponse::DynamicToolResult(_)) => {
            Ok(())
        }
        (UserActionParams::Question(params), UserActionResponse::Question(answer)) => {
            match answer {
                QuestionAnswer::SelectedOption { option_id } => {
                    if !params.options.iter().any(|option| option.id == *option_id) {
                        return Err(TalosError::public(
                            talos_error::ErrorCode::InvalidRequest,
                            format!("question option `{option_id}` does not exist"),
                        ));
                    }
                }
                QuestionAnswer::FreeForm { text } => {
                    if text.trim().is_empty() || text.len() > 4096 {
                        return Err(TalosError::public(
                            talos_error::ErrorCode::InvalidRequest,
                            "question free-form answer must contain 1 to 4096 bytes",
                        ));
                    }
                }
            }
            Ok(())
        }
        (
            UserActionParams::SandboxApproval(params),
            UserActionResponse::SandboxApproval(decision),
        ) => {
            if !params.available_decisions.contains(decision) {
                return Err(TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    format!("sandbox approval decision `{decision:?}` is not available"),
                ));
            }
            Ok(())
        }
        _ => Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "user action response variant does not match its parameters",
        )),
    }
}
