use super::TalosService;
use crate::{Actor, ServiceResult};
use talos_agent_api::{AgentInterruptOutcome, InterruptTurnRequest, InterruptTurnResponse};
use talos_error::TalosError;

impl TalosService {
    pub async fn cancel_active_turn(
        &self,
        actor: &Actor,
        session_id: &str,
        turn_id: &str,
    ) -> ServiceResult<AgentInterruptOutcome> {
        let session = self.access.load_session(actor, session_id).await?;
        if self
            .turns
            .canonical_active_turn(session_id)
            .await?
            .is_some_and(|active| active.identity.turn_id != turn_id)
        {
            return Err(TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                format!("active turn `{turn_id}` for session `{session_id}`"),
            ));
        }
        self.turns.interrupt_canonical_turn(&session, turn_id).await
    }

    pub async fn interrupt_turn(
        &self,
        actor: &Actor,
        request: InterruptTurnRequest,
    ) -> ServiceResult<InterruptTurnResponse> {
        let identity = request.identity;
        let outcome = self
            .cancel_active_turn(actor, &identity.session_id, &identity.turn_id)
            .await?;
        Ok(InterruptTurnResponse { identity, outcome })
    }
}
