mod service;
mod types;

pub use agent_protocol::{
    CreateNodeRequest, GetNodeRequest, ListNodesRequest, RotateCredentialRequest, UpdateNodeRequest,
};
pub use service::{AgentNodeRevocationHook, AgentNodeService};
pub use types::NodePrincipal;

#[cfg(test)]
#[path = "agent_node_tests.rs"]
mod tests;

#[cfg(test)]
mod computers_tests;

mod remote_management;
pub use remote_management::ComputerManagement;
