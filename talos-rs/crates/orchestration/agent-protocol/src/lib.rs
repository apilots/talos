//! Shared Agent execution and node domain contracts.
//!
//! The existing product messages and events remain in `talos-agent-api`.

mod capability;
mod checkpoint;
mod context;
pub mod dream_pb;
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
