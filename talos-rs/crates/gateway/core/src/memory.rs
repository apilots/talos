use crate::gateway_service::ServiceGateway;
use talos_error::TalosResult;
use talos_service::Actor;
use talos_types::*;
impl ServiceGateway {
    pub(crate) async fn list_memory_files(
        &self,
        actor: &Actor,
        request: MemoryFileListRequest,
    ) -> TalosResult<MemoryFileListResponse> {
        self.service.list_memory_files(actor, request).await
    }
    pub(crate) async fn read_memory_file(
        &self,
        actor: &Actor,
        request: MemoryFileReadRequest,
    ) -> TalosResult<MemoryFileReadResponse> {
        self.service.read_memory_file(actor, request).await
    }
    pub(crate) async fn write_memory_file(
        &self,
        actor: &Actor,
        request: MemoryFileWriteRequest,
    ) -> TalosResult<MemoryFileWriteResponse> {
        self.service.write_memory_file(actor, request).await
    }
    pub(crate) async fn search_memory(
        &self,
        actor: &Actor,
        request: MemorySearchRequest,
    ) -> TalosResult<MemorySearchResponse> {
        self.service.search_memory(actor, request).await
    }
    pub(crate) async fn get_memory(
        &self,
        actor: &Actor,
        request: MemoryGetRequest,
    ) -> TalosResult<MemoryGetResponse> {
        self.service.get_memory(actor, request).await
    }
}
