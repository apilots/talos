mod service;
mod types;

pub use agent_protocol::{
    CreateNodeRequest, GetNodeRequest, ListNodesRequest, RotateCredentialRequest, UpdateNodeRequest,
};
pub use service::{AgentNodeRevocationHook, AgentNodeService};
/// The existing transport owner atomically verifies Worker idleness and revokes remote admission.
pub type AgentNodeDeletionHook = std::sync::Arc<
    dyn Fn(bool) -> agent_protocol::AgentFuture<'static, talos_error::TalosResult<()>>
        + Send
        + Sync,
>;
pub use types::NodePrincipal;

#[cfg(test)]
#[path = "agent_node_tests.rs"]
mod tests;

#[cfg(test)]
mod computers_tests;

mod remote_management;
pub use remote_management::ComputerManagement;

#[cfg(test)]
mod deletion_tests;
