mod service;
mod types;

pub use agent_protocol::{
    CreateNodeRequest, GetNodeRequest, ListNodesRequest, RotateCredentialRequest, UpdateNodeRequest,
};
pub use service::AgentNodeService;
pub use types::NodePrincipal;

#[cfg(test)]
#[path = "agent_node_tests.rs"]
mod tests;
