//! Forward live product operations to the actual Turn coordinator.
//! Business admission, queue state and authorization remain in original Services.
use super::*;

impl TalosService {
    pub fn set_active_session_commands(
        &self,
        commands: Arc<dyn agent_protocol::ActiveSessionCommands>,
    ) {
        *self.active_commands.write().expect("Active commands lock") = Some(commands);
    }

    pub(super) async fn forward_active_command(
        &self,
        actor: &Actor,
        session: &SessionRecord,
        command: agent_protocol::ActiveSessionCommand,
    ) -> ServiceResult<Option<agent_protocol::ActiveSessionReply>> {
        let Some(target) = self.sessions.remote_active_target(session).await? else {
            return Ok(None);
        };
        let client = self
            .active_commands
            .read()
            .expect("Active commands lock")
            .clone()
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ServiceUnavailable,
                    "Current Turn coordinator is unavailable.",
                )
            })?;
        client
            .invoke(&actor.user_id, target, command)
            .await
            .map(Some)
    }

    /// Source-side dispatch after Gateway account authentication. Only this
    /// Service's real active claim may accept a command; imported display facts
    /// never authorize an alternate executor or a process-local queue copy.
    pub async fn invoke_active_session_command(
        &self,
        actor: &Actor,
        target: agent_protocol::ActiveSessionTarget,
        command: agent_protocol::ActiveSessionCommand,
    ) -> ServiceResult<agent_protocol::ActiveSessionReply> {
        command.validate_target(&target)?;
        let session = self
            .access
            .load_session(actor, &target.identity.session_id)
            .await?;
        if self.sessions.execution_computer_id(&session).await? != target.computer_id
            || !self.sessions.is_current_coordinator(&target.coordinator_id)
            || self
                .turns
                .canonical_active_turn(&session.id)
                .await?
                .is_none_or(|active| active.identity != target.identity)
        {
            return Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Turn is no longer owned by this coordinator.",
            ));
        }
        use agent_protocol::{ActiveSessionCommand as Command, ActiveSessionReply as Reply};
        match command {
            Command::Input(request) => self
                .submit_turn_input(actor, request)
                .await
                .map(Reply::Input),
            Command::Interrupt(request) => self
                .interrupt_turn(actor, request)
                .await
                .map(Reply::Interrupt),
            Command::AddMessage(request) => self
                .add_queued_message(actor, request)
                .await
                .map(Reply::Queue),
            Command::ListMessages(request) => self
                .list_message_queue(actor, request)
                .await
                .map(Reply::Queue),
            Command::UpdateMessage(request) => self
                .update_queued_message(actor, request)
                .await
                .map(Reply::Queue),
            Command::DeleteMessage(request) => self
                .delete_queued_message(actor, request)
                .await
                .map(Reply::Queue),
            Command::ReorderMessages(request) => self
                .reorder_message_queue(actor, request)
                .await
                .map(Reply::Queue),
            Command::StartMessage(request) => self
                .start_queued_message(actor, request)
                .await
                .map(Reply::Started),
        }
    }
}
