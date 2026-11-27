use super::*;
use talos_agent_api::{
    ActiveTurnState, AgentEventPayload as CanonicalAgentEventPayload, AgentMessageBlockContent,
    AgentMessageRole, AgentPendingInput, AgentUserContent,
};
use talos_store::session::{AgentActiveTurnRecord, AgentEventRecord};
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
            TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                format!("user action {action_id}"),
            )
        })?;
        validate_user_action_response(&action.params, &response)?;

        if let Some(existing) = submitted_user_action_response(&events, action_id) {
            if existing != &response {
                return Err(TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    format!("user action `{action_id}` already has a different response"),
                ));
            }
            self.release_command(command).await;
            return self.user_action_state(&identity, &events, &response).await;
        }

        let ownership = self.pin_session(&identity.session_id)?;
        self.canonical_active_turn(&identity.session_id)
            .await?
            .filter(|active| {
                active.identity == identity
                    && active.state == ActiveTurnState::WaitingForInput
                    && active.current_action_id.as_deref() == Some(action_id)
            })
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    "The pending user action identity does not match the active turn.",
                )
            })?;
        let context = self
            .execution
            .sessions
            .build_agent_context(&session, identity.clone())
            .await?;
        let message = crate::turn::completed_user_action_message(
            turn_id,
            &format!("msg_action_{action_id}"),
            action_id,
            response.clone(),
            command.as_ref().map_or_else(
                || chrono::Utc::now().timestamp(),
                |command| command.created_at / 1_000,
            ),
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
        self.complete_resumed_turn(session, identity, completion, ownership);
        Ok(UserActionState::Resuming)
    }
}

pub(super) fn recover_erased(
    service: TurnService,
    session: talos_store::session::SessionRecord,
    active: AgentActiveTurnRecord,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = TalosResult<()>> + Send>> {
    Box::pin(async move {
        service
            .recover_active_turn_with_dispatch(session, active, /*dispatch*/ false)
            .await
    })
}

impl TurnService {
    pub(crate) async fn recover_active_turn(
        &self,
        session: talos_store::session::SessionRecord,
        active: AgentActiveTurnRecord,
    ) -> TalosResult<()> {
        self.recover_active_turn_with_dispatch(session, active, /*dispatch*/ true)
            .await
    }

    async fn recover_active_turn_with_dispatch(
        &self,
        session: talos_store::session::SessionRecord,
        active: AgentActiveTurnRecord,
        dispatch: bool,
    ) -> TalosResult<()> {
        let service = self.clone();
        super::await_admission(async move {
            service
                .recover_active_turn_owned(session, active, dispatch)
                .await
        })
        .await
    }

    async fn recover_active_turn_owned(
        &self,
        session: talos_store::session::SessionRecord,
        _active: AgentActiveTurnRecord,
        dispatch: bool,
    ) -> TalosResult<()> {
        let context = self
            .execution
            .sessions
            .build_agent_session_context(&session)
            .await?;
        self.execution.project_worker(&context).await?;
        let state = self
            .execution
            .coordinator
            .read_session_execution_state(context)
            .await?
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ServiceUnavailable,
                    "Worker execution state is unavailable.",
                )
            })?;
        let projected = self.canonical_active_turn(&session.id).await?;
        if projected.as_ref().map(|turn| &turn.identity) != state.active_turn.as_ref() {
            return Err(TalosError::new(
                talos_error::ErrorCode::ServiceUnavailable,
                "Product execution view is awaiting Worker facts.",
            ));
        }
        if !self.execution.has_live_execution(&session.id) {
            self.recover_worker_settlements(&session.id, dispatch)
                .await?;
        }
        Ok(())
    }

    async fn canonical_user_action_events(
        &self,
        identity: &AgentTurnRef,
        action_id: &str,
    ) -> TalosResult<Vec<AgentEventRecord>> {
        let tail = self
            .execution
            .execution_store
            .get_latest_agent_event_seq(&identity.session_id)
            .await?
            .unwrap_or_default();
        let mut events = Vec::new();
        crate::timeline::scan_turn_events(
            self.execution.execution_store.as_ref(),
            identity,
            tail,
            |event| {
                if matches!(
                    &event.payload,
                    CanonicalAgentEventPayload::TurnCompleted(_)
                        | CanonicalAgentEventPayload::TurnFailed(_)
                ) || matches!(&event.payload, CanonicalAgentEventPayload::WaitingForInput(pending)
                    if pending.action_id() == action_id)
                    || submitted_user_action_response(std::slice::from_ref(&event), action_id)
                        .is_some()
                {
                    events.push(event);
                }
                Ok(())
            },
        )
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
                        return Err(TalosError::new(
                            talos_error::ErrorCode::InvalidRequest,
                            format!("question option `{option_id}` does not exist"),
                        ));
                    }
                }
                QuestionAnswer::FreeForm { text } => {
                    if text.trim().is_empty() || text.len() > 4096 {
                        return Err(TalosError::new(
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
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    format!("sandbox approval decision `{decision:?}` is not available"),
                ));
            }
            Ok(())
        }
        _ => Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "user action response variant does not match its parameters",
        )),
    }
}
