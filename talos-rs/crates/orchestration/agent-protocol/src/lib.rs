//! Shared Agent execution and node domain contracts.
//!
//! The existing product messages and events remain in `talos-agent-api`.

pub mod automation_pb;
mod capability;
mod checkpoint;
pub mod connection_pb;
mod context;
pub mod desktop_pb;
mod session_title;
pub mod session_title_pb;
pub use session_title::{
    AgentSessionTitleMode, AgentSessionTitleOutcome, AgentSessionTitleParams,
    AgentSessionTitleResult,
};
mod dream;
pub mod dream_pb;
pub use dream::{DreamMemoryCursor, DreamMemoryFormation, DreamMemoryParams, DreamMemoryResult};
mod environment;
mod environment_browser_pb;
mod environment_config_pb;
mod environment_docs_pb;
mod environment_memory_pb;
mod environment_oauth;
mod environment_oauth_pb;
pub mod environment_pb;
mod environment_plugin_changes;
mod environment_plugin_changes_pb;
mod environment_plugins_pb;
mod environment_skills_pb;
pub mod error_pb;
pub mod harness_pb;
pub mod memory_pb;
mod node;
pub mod node_pb;
mod port;
pub mod resources_pb;
mod run;

pub type AgentResult<T> = talos_error::TalosResult<T>;

pub use capability::{AgentCapabilities, AgentUserActionKind, AgentUserContentKind};
pub use checkpoint::AgentRunCheckpoint;
pub use context::{AgentContext, AgentSessionContext};
pub use node::{
    heartbeat_is_recent, CreateNodeRequest, CreatedNode, Deployment, GetNodeRequest,
    HeartbeatParams, HeartbeatResult, LeaseValidity, ListNodesRequest, NodeAuth, NodeCredential,
    NodeHealth, NodeInfo, NodeMode, NodePage, NodeRequest, NodeStatus, RegisterParams,
    Registration, RegistrationAdmission, RotateCredentialRequest, UpdateNodeRequest,
};
pub use port::{AgentFuture, AgentHarness};
pub use run::{
    AgentAdmission, AgentAdmissionPermit, AgentAdmissionUnconfirmed, AgentCommandIdentity,
    AgentInputParams, AgentInterruptParams, AgentProgressEventReceiver, AgentRunExecution,
    AgentRunIntent, AgentRunParams, AgentRunSettlement, AgentRunSettlementReceiver,
};

pub use environment::{WorkerEnvironment, WorkerEnvironmentContext, WorkerWorktreeRequest};
pub use environment_plugin_changes::{
    WorkerPluginChange, WorkerPluginInstall, WorkerPluginPrepared, WorkerPluginUpdate,
};

pub use environment_oauth::{
    McpAuthenticationCallbackParams, McpAuthenticationStartParams, McpOAuthCallbackOutcome,
    McpOAuthCallbackTarget,
};

mod coordinator_commands;
mod execution_state;
pub use coordinator_commands::{
    ActiveSessionCommand, ActiveSessionCommands, ActiveSessionReply, ActiveSessionTarget,
};
pub mod execution_state_pb;
pub use execution_state::{SessionExecutionState, SessionHistoryCursor};

mod execution_events;
pub use execution_events::{
    AgentCommittedEvent, AgentExecutionCommand, AgentExecutionPayloadParams,
    AgentExecutionPayloadResult, AgentExecutionPurpose, AgentExecutionSettlement,
    AgentMemoryModelUsage, AgentSessionEventCursor, AgentSessionEventsParams,
    AgentSessionEventsResult, MAX_EXECUTION_CAPTURE_BYTES, MAX_EXECUTION_EVENT_BYTES,
    MAX_EXECUTION_PAGE_BYTES, MAX_EXECUTION_PAGE_EVENTS,
};

pub mod execution_events_pb;

mod node_admission;
pub use node_admission::NodeAdmissionGates;

/// Result of the existing managed Worker wake admission policy.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ManagedDispatch {
    Proceed,
    Waking,
    Deferred,
}
