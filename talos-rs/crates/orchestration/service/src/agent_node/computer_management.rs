//! Authenticated control-plane client used by personal product composition.
use crate::Actor;
use agent_protocol::AgentFuture;
use talos_error::TalosResult;
use talos_types::{
    CreateNodeRequest, CreatedNode, NodeCredential, NodeInfo, RenameComputerRequest,
    RotateCredentialRequest, SetComputerStatusRequest, UpdateNodeRequest,
};

/// The application supplies the same authenticated Node SDK used for enrollment.
/// This port never persists a replica lease or creates a second registry.
pub trait ComputerManagement: Send + Sync {
    /// Reads a validated account directory without owning a second node Store.
    fn list<'a>(
        &'a self,
        actor: &'a Actor,
        request: talos_types::ComputerListRequest,
    ) -> AgentFuture<'a, TalosResult<talos_types::ComputerListResponse>>;
    fn read<'a>(
        &'a self,
        actor: &'a Actor,
        request: talos_types::GetNodeRequest,
    ) -> AgentFuture<'a, TalosResult<NodeInfo>>;
    fn delete<'a>(
        &'a self,
        actor: &'a Actor,
        request: talos_types::DeleteComputerRequest,
    ) -> AgentFuture<'a, TalosResult<talos_types::DeleteComputerResponse>>;
    fn create<'a>(
        &'a self,
        actor: &'a Actor,
        request: CreateNodeRequest,
    ) -> AgentFuture<'a, TalosResult<CreatedNode>>;
    fn update<'a>(
        &'a self,
        actor: &'a Actor,
        request: UpdateNodeRequest,
    ) -> AgentFuture<'a, TalosResult<NodeInfo>>;
    fn rename<'a>(
        &'a self,
        actor: &'a Actor,
        request: RenameComputerRequest,
    ) -> AgentFuture<'a, TalosResult<NodeInfo>>;
    fn set_status<'a>(
        &'a self,
        actor: &'a Actor,
        request: SetComputerStatusRequest,
    ) -> AgentFuture<'a, TalosResult<NodeInfo>>;
    fn rotate_credential<'a>(
        &'a self,
        actor: &'a Actor,
        request: RotateCredentialRequest,
    ) -> AgentFuture<'a, TalosResult<NodeCredential>>;
}
