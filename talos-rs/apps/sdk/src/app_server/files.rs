use base64::Engine;
use bytes::Bytes;
use talos_app_protocol::requests::FileUploadParams;
use talos_types::FileUploadRequest;

pub(super) fn file_upload_params(request: FileUploadRequest, data: Bytes) -> FileUploadParams {
    FileUploadParams {
        request,
        data_base64: base64::engine::general_purpose::STANDARD.encode(data),
    }
}
