//! Shared Agent execution and node domain contracts.
//!
//! The existing product messages and events remain in `talos-agent-api`.

mod capability;
mod checkpoint;
pub mod connection_pb;
mod context;
pub mod desktop_pb;
pub mod dream_pb;
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
    Registration, RotateCredentialRequest, UpdateNodeRequest,
};
pub use port::{AgentFuture, AgentHarness, DreamMemoryModelCall};
pub use run::{
    AgentAdmission, AgentAdmissionPermit, AgentInputParams, AgentInterruptParams,
    AgentProgressEventReceiver, AgentRunExecution, AgentRunParams, AgentRunSettlement,
    AgentRunSettlementReceiver,
};

pub use environment::{WorkerEnvironment, WorkerEnvironmentContext};
pub use environment_plugin_changes::{
    WorkerPluginChange, WorkerPluginInstall, WorkerPluginPrepared, WorkerPluginUpdate,
};

pub use environment_oauth::{
    McpAuthenticationCallbackParams, McpAuthenticationStartParams, McpOAuthCallbackOutcome,
    McpOAuthCallbackTarget,
};

mod coordinator_commands;
pub mod coordinator_commands_pb;
mod execution_state;
pub use coordinator_commands::{
    ActiveSessionCommand, ActiveSessionCommands, ActiveSessionReply, ActiveSessionTarget,
};
pub mod execution_state_pb;
pub use execution_state::{
    CommitSessionExecutionStateParams, SessionExecutionState, SessionHistoryCursor,
    SessionStateVisibility,
};
