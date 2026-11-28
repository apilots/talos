mod worker_projection;
pub use worker_projection::{
    WorkerProjectionCommit, WorkerProjectionParams, WorkerSettlementRecord,
};
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
    AgentActiveTurnRecord, AgentCancellationAdmission, AgentEventRecord, AgentExecutionStore,
    CommandLease, CommandReservation, IdempotencyRecord, IdempotencyReservation, IdempotentCommand,
    TimelineEventRecord,
};
pub use mock_store::MockSessionStore;
pub use store::{DatabaseSessionStore, SessionStore};
pub use types::{
    AccumulateSessionUsageParams, PatchSessionParams, SessionPageCursor, SessionQuery,
    SessionRecord, SessionRecordPatch, SessionSummary, SessionTreeIdentity, SessionTreeScope,
};
