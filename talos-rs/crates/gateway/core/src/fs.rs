use talos_application_service::Actor;
use talos_error::TalosResult;
use talos_types::{
    FsDirectoryCreateRequest, FsDirectoryCreateResponse, FsDirectoryReadRequest,
    FsDirectoryReadResponse,
};

use crate::application_service_gateway::ApplicationServiceGateway;

impl ApplicationServiceGateway {
    pub(crate) async fn read_fs_directory(
        &self,
        actor: &Actor,
        request: FsDirectoryReadRequest,
    ) -> TalosResult<FsDirectoryReadResponse> {
        let directory = self
            .application_service
            .read_fs_directory(actor, request.path)
            .await?;
        Ok(FsDirectoryReadResponse { directory })
    }

    pub(crate) async fn create_fs_directory(
        &self,
        actor: &Actor,
        request: FsDirectoryCreateRequest,
    ) -> TalosResult<FsDirectoryCreateResponse> {
        self.application_service
            .create_fs_directory(actor, request.parent, request.name)
            .await
    }
}
