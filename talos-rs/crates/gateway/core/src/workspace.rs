use talos_error::TalosResult;
use talos_service::Actor;
use talos_types::{
    WorkspaceDocListRequest, WorkspaceDocListResponse, WorkspaceDocReadRequest,
    WorkspaceDocReadResponse, WorkspaceDocWriteRequest, WorkspaceDocWriteResponse,
    WorkspaceInfoRequest, WorkspaceInfoResponse,
};

use crate::gateway_service::ServiceGateway;

impl ServiceGateway {
    pub(crate) async fn workspace_info(
        &self,
        actor: &Actor,
        request: WorkspaceInfoRequest,
    ) -> TalosResult<WorkspaceInfoResponse> {
        self.service.workspace_info(actor, request).await
    }

    pub(crate) async fn list_workspace_docs(
        &self,
        actor: &Actor,
        request: WorkspaceDocListRequest,
    ) -> TalosResult<WorkspaceDocListResponse> {
        self.service.list_workspace_docs(actor, request).await
    }
    pub(crate) async fn read_workspace_doc(
        &self,
        actor: &Actor,
        request: WorkspaceDocReadRequest,
    ) -> TalosResult<WorkspaceDocReadResponse> {
        self.service.read_workspace_doc(actor, request).await
    }
    pub(crate) async fn write_workspace_doc(
        &self,
        actor: &Actor,
        request: WorkspaceDocWriteRequest,
    ) -> TalosResult<WorkspaceDocWriteResponse> {
        self.service.write_workspace_doc(actor, request).await
    }
}
