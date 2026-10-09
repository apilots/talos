use super::*;

impl TurnService {
    pub(crate) async fn validate_subagent_agent(
        &self,
        agent_id: &str,
        user_id: &str,
        node_id: &str,
    ) -> TalosResult<()> {
        self.execution.coordinator.ensure_agent(agent_id).await?;
        self.execution
            .coordinator
            .select_execution_node(agent_id, user_id, Some(node_id))
            .await
            .map(|_| ())
    }

    pub(crate) async fn submit_subagent_turn(
        &self,
        session: talos_store::session::SessionRecord,
        request: StartTurnRequest,
        agent_id: String,
    ) -> TalosResult<StartTurnResponse> {
        Self::validate_start_turn_request(&request)?;
        if session.agent_id != agent_id {
            return Err(
                TalosError::internal("Session is not bound to Agent.").context(format!(
                    "session '{}' is not bound to Agent '{agent_id}'",
                    session.id
                )),
            );
        }
        self.submit_turn(
            session,
            TurnSubmission {
                input: request.input,
                retry_of_message_id: None,
            },
            None,
            None,
            None,
        )
        .await
    }

    pub(crate) async fn interrupt_subagent_turn(
        &self,
        session: talos_store::session::SessionRecord,
        turn_id: String,
        agent_id: String,
    ) -> TalosResult<()> {
        if session.agent_id != agent_id {
            return Err(
                TalosError::internal("Session is not bound to Agent.").context(format!(
                    "session '{}' is not bound to Agent '{agent_id}'",
                    session.id
                )),
            );
        }
        match self.interrupt_canonical_turn(&session, &turn_id).await? {
            talos_agent_api::AgentInterruptOutcome::Cancelled
            | talos_agent_api::AgentInterruptOutcome::Completed => Ok(()),
            talos_agent_api::AgentInterruptOutcome::CleanupUnconfirmed { task_count } => {
                Err(TalosError::new(
                    talos_error::ErrorCode::RunCleanupUnconfirmed,
                    format!("Subagent cleanup is unconfirmed for {task_count} task(s)"),
                ))
            }
        }
    }

    pub(crate) async fn submit_subagent_message(
        &self,
        session: talos_store::session::SessionRecord,
        turn_id: String,
        agent_id: String,
        message: String,
    ) -> TalosResult<()> {
        if session.agent_id != agent_id {
            return Err(
                TalosError::internal("Session is not bound to Agent.").context(format!(
                    "session '{}' is not bound to Agent '{agent_id}'",
                    session.id
                )),
            );
        }
        let identity = AgentTurnRef::new(&session.id, &turn_id);
        let message_id = format!("msg_{}", Uuid::new_v4().simple());
        let input = crate::turn::completed_agent_user_input(
            &turn_id,
            &message_id,
            AgentUserInput {
                content: vec![AgentUserContent::Text { text: message }],
            },
            chrono::Utc::now().timestamp(),
        )?;
        self.execution
            .admit_running_input(
                AgentInputParams {
                    agent_id,
                    identity,
                    input,
                },
                None,
            )
            .await
    }
}
