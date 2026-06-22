use talos_application_service::Actor;
use talos_error::TalosResult;
use talos_types::{
    MemoryFileListRequest, MemoryFileListResponse, MemoryFileReadRequest, MemoryFileReadResponse,
    MemoryFileWriteRequest, MemoryFileWriteResponse, MemorySearchRequest, MemorySearchResponse,
};

use crate::application_service_gateway::ApplicationServiceGateway;

impl ApplicationServiceGateway {
    pub(crate) async fn list_memory_files(
        &self,
        actor: &Actor,
        request: MemoryFileListRequest,
    ) -> TalosResult<MemoryFileListResponse> {
        self.application_service
            .memory
            .list_files_with_request(actor, request)
            .await
    }

    pub(crate) async fn read_memory_file(
        &self,
        actor: &Actor,
        request: MemoryFileReadRequest,
    ) -> TalosResult<MemoryFileReadResponse> {
        let file = self
            .application_service
            .memory
            .read_file_with_request(actor, request)
            .await?;
        Ok(MemoryFileReadResponse { file })
    }

    pub(crate) async fn write_memory_file(
        &self,
        actor: &Actor,
        request: MemoryFileWriteRequest,
    ) -> TalosResult<MemoryFileWriteResponse> {
        let file = self
            .application_service
            .memory
            .write_file_with_request(actor, request)
            .await?;
        Ok(MemoryFileWriteResponse { file })
    }

    pub(crate) async fn search_memory(
        &self,
        actor: &Actor,
        request: MemorySearchRequest,
    ) -> TalosResult<MemorySearchResponse> {
        self.application_service
            .memory
            .search_files_with_request(actor, request)
            .await
    }
}
