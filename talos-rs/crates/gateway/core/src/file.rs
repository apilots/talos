use base64::Engine;
use talos_error::TalosResult;
use talos_service::{Actor, FileByteStream};
use talos_types::{
    FileDeleteRequest, FileDeleteResponse, FileListRequest, FileListResponse, FileReadRequest,
    FileReadResponse, FileUpdateRequest, FileUpdateResponse, FileUploadRequest, FileUploadResponse,
};

use crate::ServiceGateway;

impl ServiceGateway {
    pub(crate) async fn upload_file(
        &self,
        actor: &Actor,
        request: FileUploadRequest,
        body: FileByteStream,
    ) -> TalosResult<FileUploadResponse> {
        self.service.files.upload(actor, request, body).await
    }

    pub(crate) async fn list_files(
        &self,
        actor: &Actor,
        request: FileListRequest,
    ) -> TalosResult<FileListResponse> {
        self.service.files.list(actor, request).await
    }

    pub(crate) async fn read_file(
        &self,
        actor: &Actor,
        request: FileReadRequest,
    ) -> TalosResult<FileReadResponse> {
        self.service.files.read(actor, request).await
    }

    pub(crate) async fn read_file_content(
        &self,
        actor: &Actor,
        request: FileReadRequest,
    ) -> TalosResult<talos_types::FileContentResponse> {
        let content = self.service.files.read_content(actor, request).await?;
        Ok(talos_types::FileContentResponse {
            file: content.file,
            data_base64: base64::engine::general_purpose::STANDARD.encode(content.bytes),
        })
    }

    pub(crate) async fn update_file(
        &self,
        actor: &Actor,
        request: FileUpdateRequest,
    ) -> TalosResult<FileUpdateResponse> {
        self.service.files.update(actor, request).await
    }

    pub(crate) async fn delete_file(
        &self,
        actor: &Actor,
        request: FileDeleteRequest,
    ) -> TalosResult<FileDeleteResponse> {
        self.service.files.delete(actor, request).await
    }
}
