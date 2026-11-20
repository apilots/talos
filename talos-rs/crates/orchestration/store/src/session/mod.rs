pub mod agent_execution;
#[path = "store/local_registration.rs"]
mod local_registration;
mod mock_store;
mod store;
pub(crate) use local_registration::capture_removal_boundary;
mod types;

#[cfg(test)]
mod scope_tests;

pub use agent_execution::{
    AgentActiveTurnRecord, AgentCancellationAdmission, AgentContinuationBindingRecord,
    AgentContinuationBindingStatus, AgentEventCommit, AgentEventRecord, AgentExecutionStore,
    ClaimContinuationParams, CommandLease, CommandReservation, CommitInitialAdmissionParams,
    CommitRunningInputAdmissionParams, CommitTerminalSettlementParams, IdempotencyRecord,
    IdempotencyReservation, IdempotentCommand, TimelineEventRecord,
};
pub use mock_store::MockSessionStore;
pub use store::{DatabaseSessionStore, SessionStore};
pub use types::{
    AccumulateSessionUsageParams, ComputerDataScope, MaintenanceDataScope, PatchSessionParams,
    SessionPageCursor, SessionQuery, SessionRecord, SessionSummary, SessionTreeIdentity,
    SessionTreeScope, UnassignedComputerData,
};

macro_rules! session_computer_predicate {
    ($prefix:literal) => {
        concat!(
            $prefix,
            "((sessions.execution_node_id = 'embedded' AND coalesce(sessions.user_id = ?, 0))
              OR EXISTS (SELECT 1 FROM agent_nodes
                  WHERE agent_nodes.node_id = sessions.execution_node_id
                    AND agent_nodes.owner_user_id = sessions.user_id
                    AND agent_nodes.credential_hash IS NOT NULL))"
        )
    };
}

pub(crate) const REGISTERED_SESSION_COMPUTER: &str = session_computer_predicate!("");
const UNASSIGNED_SESSION_COMPUTER: &str = session_computer_predicate!("NOT ");
