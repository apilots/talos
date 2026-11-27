use crate::ServiceGateway;
use talos_error::TalosResult;
use talos_service::Actor;
use talos_types::{DeleteManagedWorkerRequest, ManagedWorkerActionRequest, ManagedWorkerResponse};

impl ServiceGateway {
    pub(crate) async fn delete_computer(
        &self,
        actor: &Actor,
        request: talos_types::DeleteComputerRequest,
    ) -> TalosResult<talos_types::DeleteComputerResponse> {
        self.service.nodes.delete_computer(actor, request).await
    }

    pub(crate) async fn list_computers(
        &self,
        actor: &Actor,
        request: talos_types::ComputerListRequest,
    ) -> TalosResult<talos_types::ComputerListResponse> {
        self.service.list_computers(actor, request).await
    }
    pub(crate) async fn read_computer(
        &self,
        actor: &Actor,
        request: talos_types::GetNodeRequest,
    ) -> TalosResult<talos_types::NodeInfo> {
        self.service.read_computer(actor, request).await
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
    pub(crate) async fn read_managed_worker(
        &self,
        actor: &Actor,
    ) -> TalosResult<ManagedWorkerResponse> {
        self.service.nodes.read_managed_worker(actor).await
    }

    pub(crate) async fn create_managed_worker(
        &self,
        actor: &Actor,
    ) -> TalosResult<ManagedWorkerResponse> {
        let nodes = &self.service.nodes;
        nodes
            .create_managed_worker(actor)
            .await
            .map(managed_worker_response)
    }

    pub(crate) async fn start_managed_worker(
        &self,
        actor: &Actor,
        request: ManagedWorkerActionRequest,
    ) -> TalosResult<ManagedWorkerResponse> {
        let nodes = &self.service.nodes;
        nodes
            .start_managed_worker(actor, request)
            .await
            .map(managed_worker_response)
    }

    pub(crate) async fn stop_managed_worker(
        &self,
        actor: &Actor,
        request: ManagedWorkerActionRequest,
    ) -> TalosResult<ManagedWorkerResponse> {
        let nodes = &self.service.nodes;
        nodes
            .stop_managed_worker(actor, request)
            .await
            .map(managed_worker_response)
    }

    pub(crate) async fn reset_managed_worker(
        &self,
        actor: &Actor,
        request: ManagedWorkerActionRequest,
    ) -> TalosResult<ManagedWorkerResponse> {
        let nodes = &self.service.nodes;
        nodes
            .reset_managed_worker(actor, request)
            .await
            .map(managed_worker_response)
    }

    pub(crate) async fn delete_managed_worker(
        &self,
        actor: &Actor,
        request: DeleteManagedWorkerRequest,
    ) -> TalosResult<ManagedWorkerResponse> {
        let nodes = &self.service.nodes;
        nodes
            .delete_managed_worker(actor, request)
            .await
            .map(managed_worker_response)
    }
}

pub(crate) fn managed_worker_response(
    instance: talos_types::ManagedWorkerInfo,
) -> talos_types::ManagedWorkerResponse {
    talos_types::ManagedWorkerResponse {
        enabled: true,
        instance: Some(instance),
    }
}
