//! Closed product commands directed to the current Turn's original Service.
//! Requests and replies reuse the existing product domain types. This port owns
//! no admission, queue, claim, history or credential persistence.
use crate::AgentFuture;
use talos_agent_api::{
    InterruptTurnRequest, InterruptTurnResponse, SubmitTurnInputRequest, SubmitTurnInputResponse,
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;

#[derive(Debug, Clone)]
pub struct ActiveSessionTarget {
    pub computer_id: String,
    pub coordinator_id: String,
    pub identity: AgentTurnRef,
}

pub enum ActiveSessionCommand {
    Input(SubmitTurnInputRequest),
    Interrupt(InterruptTurnRequest),
    AddMessage(MessageQueueAddRequest),
    ListMessages(MessageQueueListRequest),
    UpdateMessage(MessageQueueUpdateRequest),
    DeleteMessage(MessageQueueDeleteRequest),
    ReorderMessages(MessageQueueReorderRequest),
    StartMessage(MessageQueueStartRequest),
}
impl ActiveSessionCommand {
    /// Validate the immutable destination before transport or Service dispatch.
    pub fn validate_target(&self, target: &ActiveSessionTarget) -> TalosResult<()> {
        let matches = match self {
            Self::Input(request) => request.identity == target.identity,
            Self::Interrupt(request) => request.identity == target.identity,
            Self::AddMessage(request) => request.session_id == target.identity.session_id,
            Self::ListMessages(request) => request.session_id == target.identity.session_id,
            Self::UpdateMessage(request) => request.session_id == target.identity.session_id,
            Self::DeleteMessage(request) => request.session_id == target.identity.session_id,
            Self::ReorderMessages(request) => request.session_id == target.identity.session_id,
            Self::StartMessage(request) => request.session_id == target.identity.session_id,
        };
        if !matches
            || target.identity.session_id.is_empty()
            || target.identity.turn_id.is_empty()
            || uuid::Uuid::parse_str(&target.computer_id).is_err()
            || uuid::Uuid::parse_str(&target.coordinator_id).is_err()
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Active Session command destination is invalid.",
            ));
        }
        Ok(())
    }
}
pub enum ActiveSessionReply {
    Input(SubmitTurnInputResponse),
    Interrupt(InterruptTurnResponse),
    Queue(MessageQueue),
    Started(StartTurnResponse),
}
pub trait ActiveSessionCommands: Send + Sync {
    fn invoke<'a>(
        &'a self,
        owner: &'a str,
        target: ActiveSessionTarget,
        command: ActiveSessionCommand,
    ) -> AgentFuture<'a, TalosResult<ActiveSessionReply>>;
}
