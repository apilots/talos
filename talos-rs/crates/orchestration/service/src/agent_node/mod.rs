//! Product access uses local capabilities or the existing Server authority.
mod computer_management;
mod product_access;
pub use agent_protocol::{
    CreateNodeRequest, GetNodeRequest, ListNodesRequest, RotateCredentialRequest, UpdateNodeRequest,
};
pub use computer_management::ComputerManagement;
pub use product_access::ProductNodeAccess;
pub(crate) use product_access::{ManagedConversationBinding, ManagedDispatch};
#[cfg(feature = "control-plane")]
pub use talos_control_plane::{AgentNodeRevocationHook, AgentNodeService, NodePrincipal};
