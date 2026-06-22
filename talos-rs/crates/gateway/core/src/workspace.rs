use talos_application_service::Actor;
use talos_error::TalosResult;
use talos_types::{
    WorkspaceDocListRequest, WorkspaceDocListResponse, WorkspaceDocReadRequest,
    WorkspaceDocReadResponse, WorkspaceDocWriteRequest, WorkspaceDocWriteResponse,
    WorkspaceInfoRequest, WorkspaceInfoResponse,
};

use crate::application_service_gateway::ApplicationServiceGateway;

impl ApplicationServiceGateway {
    pub(crate) async fn list_workspace_docs(
        &self,
        actor: &Actor,
        request: WorkspaceDocListRequest,
    ) -> TalosResult<WorkspaceDocListResponse> {
        self.application_service
            .workspace_docs
            .list_docs_with_request(actor, request)
            .await
    }

    pub(crate) async fn read_workspace_doc(
        &self,
        actor: &Actor,
        request: WorkspaceDocReadRequest,
    ) -> TalosResult<WorkspaceDocReadResponse> {
        let doc = self
            .application_service
            .workspace_docs
            .read_doc_with_request(actor, request)
            .await?;
        Ok(WorkspaceDocReadResponse { doc })
    }

    pub(crate) async fn write_workspace_doc(
        &self,
        actor: &Actor,
        request: WorkspaceDocWriteRequest,
    ) -> TalosResult<WorkspaceDocWriteResponse> {
        let doc = self
            .application_service
            .workspace_docs
            .write_doc_with_request(actor, request)
            .await?;
        Ok(WorkspaceDocWriteResponse { doc })
    }

    pub(crate) async fn workspace_info(
        &self,
        actor: &Actor,
        request: WorkspaceInfoRequest,
    ) -> TalosResult<WorkspaceInfoResponse> {
        self.application_service
            .workspace_docs
            .workspace_info_with_request(actor, request)
    }
}
