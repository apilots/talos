use super::*;

impl TurnService {
    pub(crate) async fn validate_subagent_agent(&self, agent_id: &str) -> TalosResult<()> {
        self.executor.validate_agent_id(agent_id).await
    }

    pub(crate) async fn submit_subagent_turn(
        &self,
        session: TalosSession,
        request: StartTurnRequest,
        agent_id: String,
    ) -> TalosResult<MessageCommandResult> {
        Self::validate_start_turn_request(&request)?;
        self.submit_turn(
            session,
            TurnSubmission {
                input: request.input,
                workspace_id: None,
                binding: DispatchBinding::Exact { agent_id },
            },
            None,
        )
        .await
    }

    pub(crate) async fn interrupt_subagent_turn(
        &self,
        session: TalosSession,
        turn_id: String,
        agent_id: String,
    ) -> TalosResult<()> {
        self.session_runtime
            .cancel_active_turn(talos_types::CancelActiveTurnParams {
                binding: DispatchBinding::Exact { agent_id },
                session,
                turn_id,
            })
            .await
    }

    pub(crate) async fn submit_subagent_message(
        &self,
        session: TalosSession,
        turn_id: String,
        agent_id: String,
        message: String,
    ) -> TalosResult<()> {
        self.executor
            .submit_input(AgentDispatchInputParams {
                binding: DispatchBinding::Exact { agent_id },
                session,
                turn_id,
                input: AgentDispatchInput::Message { message },
            })
            .await
    }
}
