use crate::ServiceGateway;
use talos_error::TalosResult;
use talos_service::Actor;

impl ServiceGateway {
    pub(crate) async fn list_computers(
        &self,
        actor: &Actor,
        request: talos_types::ComputerListRequest,
    ) -> TalosResult<talos_types::ComputerListResponse> {
        self.service.nodes.list_computers(actor, request).await
    }
    pub(crate) async fn read_computer(
        &self,
        actor: &Actor,
        request: talos_types::GetNodeRequest,
    ) -> TalosResult<talos_types::NodeInfo> {
        self.service.nodes.read_computer(actor, request).await
    }
    pub(crate) async fn create_computer(
        &self,
        actor: &Actor,
        request: talos_types::CreateNodeRequest,
    ) -> TalosResult<talos_types::CreatedNode> {
        self.service.nodes.create_node(actor, request).await
    }
    pub(crate) async fn rename_computer(
        &self,
        actor: &Actor,
        request: talos_types::RenameComputerRequest,
    ) -> TalosResult<talos_types::NodeInfo> {
        self.service.nodes.rename_computer(actor, request).await
    }
    pub(crate) async fn set_computer_status(
        &self,
        actor: &Actor,
        request: talos_types::SetComputerStatusRequest,
    ) -> TalosResult<talos_types::NodeInfo> {
        self.service.nodes.set_computer_status(actor, request).await
    }
    pub(crate) async fn rotate_computer_credential(
        &self,
        actor: &Actor,
        request: talos_types::RotateCredentialRequest,
    ) -> TalosResult<talos_types::NodeCredential> {
        self.service.nodes.rotate_credential(actor, request).await
    }
}
