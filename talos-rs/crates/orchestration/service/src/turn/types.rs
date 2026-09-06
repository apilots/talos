use talos_agent_api::AgentUserInput;
use talos_store::session::CommandLease;
use talos_types::StartTurnResponse;

/// Stable identities and optional Store lease allocated before Session mutation.
pub(crate) struct PreparedTurnSubmission {
    pub request_id: String,
    pub turn_id: String,
    pub message_id: String,
    pub command: Option<CommandLease>,
}

/// Service branch produced by resolving Start idempotency before side effects.
pub(crate) enum StartTurnReservation {
    Ready(PreparedTurnSubmission),
    Completed(StartTurnResponse),
}

#[derive(Debug, Clone)]
pub(crate) struct TurnRetryParams {
    pub input: AgentUserInput,
    pub client_message_id: String,
    pub retry_of_message_id: String,
}
