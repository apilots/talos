use talos_agent_api::{
    AgentMessage, AgentMessageBlockContent, AgentPendingInput, AgentTurnRef, AgentUserContent,
};
use talos_error::TalosError;
use talos_run_control::{RunTaskCompletion, SpawnError};

use super::{ActiveInvocation, ActiveOperation, Harness};
use crate::{
    AgentAdmission, AgentAdmissionPermit, AgentInputParams, AgentProvider, AgentResult,
    AgentUserActionKind, AgentUserContentKind, SessionTurnResult,
};

impl<P> Harness<P>
where
    P: AgentProvider,
{
    /// Reserve one typed User message and return its durable-delivery permit.
    pub async fn submit_input(&self, params: AgentInputParams) -> AgentResult<AgentAdmission> {
        self.validate_agent_id(&params.agent_id)?;
        self.validate_running_input_capabilities(&params.input)?;
        if params.input.turn_id != params.identity.turn_id {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Agent input turn_id does not match Agent Turn identity",
            ));
        }
        let invocation = self.running_invocation(&params.identity)?;
        let (events, reservation) = invocation
            .pipeline
            .reserve_user_message(params.input.clone())
            .await?;
        let input = params.input;
        let input_sender = invocation.input;
        let input_failure = invocation.input_failure.clone();
        let (delivery_tx, delivery_rx) = tokio::sync::oneshot::channel();
        let (delivery_result_tx, delivery_result_rx) = tokio::sync::oneshot::channel();
        invocation
            .control
            .tasks()
            .spawn(move |_cancellation| async move {
                if delivery_rx.await.is_ok() {
                    let result = input_sender.send(input).await;
                    if let Err(error) = &result {
                        input_failure.send_replace(Some(error.clone()));
                    }
                    let _ = delivery_result_tx.send(result);
                }
                RunTaskCompletion::Confirmed(())
            })
            .map_err(input_task_error)?;
        let permit_failure = invocation.input_failure;
        let permit = AgentAdmissionPermit::new(async move {
            if let Err(error) = reservation.commit().await {
                permit_failure.send_replace(Some(error.clone()));
                return Err(error);
            }
            delivery_tx.send(()).map_err(|()| {
                let error = TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    "Agent input delivery stopped before durable admission completed",
                );
                permit_failure.send_replace(Some(error.clone()));
                error
            })?;
            delivery_result_rx.await.map_err(|_| {
                let error = TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    "Agent input delivery stopped before Provider acknowledgement",
                );
                permit_failure.send_replace(Some(error.clone()));
                error
            })?
        });
        Ok(AgentAdmission { events, permit })
    }

    pub(super) fn validate_fresh_input_capabilities(
        &self,
        input: &AgentMessage,
    ) -> AgentResult<()> {
        self.validate_content_capabilities(input)
    }

    pub(super) fn validate_continuation_input_capabilities(
        &self,
        input: &AgentMessage,
    ) -> AgentResult<()> {
        self.require_capability(
            self.inner.capabilities.durable_continuation,
            "durableContinuation",
        )?;
        self.validate_content_capabilities(input)
    }

    fn validate_running_input_capabilities(&self, input: &AgentMessage) -> AgentResult<()> {
        self.require_capability(self.inner.capabilities.dynamic_input, "dynamicInput")?;
        self.validate_content_capabilities(input)
    }

    fn validate_content_capabilities(&self, input: &AgentMessage) -> AgentResult<()> {
        for block in &input.blocks {
            let AgentMessageBlockContent::UserContent(content) = &block.content else {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "Agent input messages may contain only User content blocks",
                ));
            };
            match content {
                AgentUserContent::Text { .. } => {}
                AgentUserContent::SkillSelection { .. } => {
                    self.require_rich_input_kind(AgentUserContentKind::SkillSelection)?;
                }
                AgentUserContent::File { .. } => {
                    self.require_rich_input_kind(AgentUserContentKind::File)?;
                }
                AgentUserContent::Audio { .. } => {
                    self.require_rich_input_kind(AgentUserContentKind::Audio)?;
                }
                AgentUserContent::Video { .. } => {
                    self.require_rich_input_kind(AgentUserContentKind::Video)?;
                }
                AgentUserContent::UserActionResponse { response, .. } => {
                    let kind = match response {
                        talos_types::UserActionResponse::DynamicToolResult(_) => {
                            AgentUserActionKind::DynamicToolResult
                        }
                        talos_types::UserActionResponse::Question(_) => {
                            AgentUserActionKind::Question
                        }
                        talos_types::UserActionResponse::SandboxApproval(_) => {
                            AgentUserActionKind::SandboxApproval
                        }
                    };
                    self.require_user_action_kind(kind)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn validate_turn_result_capabilities(
        &self,
        result: &SessionTurnResult,
    ) -> AgentResult<()> {
        let SessionTurnResult::WaitingForInput { pending, .. } = result else {
            return Ok(());
        };
        self.require_capability(
            self.inner.capabilities.durable_continuation,
            "durableContinuation",
        )?;
        let AgentPendingInput::UserAction { action } = pending.as_ref() else {
            return Ok(());
        };
        let kind = match action.params {
            talos_types::UserActionParams::DynamicToolResult(_) => {
                AgentUserActionKind::DynamicToolResult
            }
            talos_types::UserActionParams::Question(_) => AgentUserActionKind::Question,
            talos_types::UserActionParams::SandboxApproval(_) => {
                AgentUserActionKind::SandboxApproval
            }
        };
        self.require_user_action_kind(kind)
    }

    fn require_capability(&self, supported: bool, capability_name: &str) -> AgentResult<()> {
        if supported {
            return Ok(());
        }
        Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!(
                "agent `{}` does not support capability `{capability_name}`",
                self.inner.agent_id
            ),
        ))
    }

    fn require_rich_input_kind(&self, kind: AgentUserContentKind) -> AgentResult<()> {
        if self.inner.capabilities.rich_input.contains(&kind) {
            return Ok(());
        }
        Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!(
                "agent `{}` does not support capability `richInput`",
                self.inner.agent_id
            ),
        ))
    }

    fn require_user_action_kind(&self, kind: AgentUserActionKind) -> AgentResult<()> {
        if self.inner.capabilities.user_actions.contains(&kind) {
            return Ok(());
        }
        Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!(
                "agent `{}` does not support capability `userAction`",
                self.inner.agent_id
            ),
        ))
    }

    fn running_invocation(&self, identity: &AgentTurnRef) -> AgentResult<ActiveInvocation> {
        let operation = self.inner.active.get(&identity.session_id).ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                "Agent input targets no live Provider invocation",
            )
        })?;
        match operation.value() {
            ActiveOperation::Running(invocation) if invocation.identity == *identity => {
                Ok(invocation.clone())
            }
            _ => Err(TalosError::new(
                talos_error::ErrorCode::ResourceNotFound,
                "Agent input targets no matching live Provider invocation",
            )),
        }
    }
}

fn input_task_error(error: SpawnError) -> TalosError {
    let message = match error {
        SpawnError::Closed => "Agent run stopped accepting input delivery",
        SpawnError::CapacityExceeded => "Agent run reached its input delivery task capacity",
    };
    TalosError::new(talos_error::ErrorCode::InternalError, message)
}
