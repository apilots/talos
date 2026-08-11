use super::*;
use crate::server::errors::ApiError;
use crate::server::files::{
    delete_file_handler, list_files_handler, read_file_handler, update_file_handler,
    upload_file_handler,
};
use axum::extract::DefaultBodyLimit;
use axum::http::header::CONTENT_TYPE;
use talos_error::TalosFileErrorKind;
use talos_types::{FileDeleteResponse, FileListResponse, FileReadResponse, FileUploadResponse};

fn file_router(state: Arc<HttpApiState>) -> Router {
    Router::new()
        .route(
            "/v1/files",
            get(list_files_handler)
                .post(upload_file_handler)
                .layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/v1/files/:file_id",
            get(read_file_handler)
                .patch(update_file_handler)
                .delete(delete_file_handler),
        )
        .layer(axum::Extension(test_auth()))
        .with_state(state)
}

#[tokio::test]
async fn file_routes_share_one_metadata_lifecycle() {
    let app = file_router(test_state(false).await);
    let response = app
        .clone()
        .oneshot(multipart_request(
            serde_json::json!({
                "filename": "diagram.png",
                "scope": { "type": "personal" },
                "tags": ["image", "diagram"],
                "metadata": { "source": "upload" }
            }),
            b"\x89PNG\r\n\x1a\n",
            false,
        ))
        .await
        .expect("upload request should complete");
    assert_eq!(response.status(), StatusCode::OK);
    let uploaded: FileUploadResponse = response_success_json(response).await;
    assert_eq!(uploaded.file.original_filename, "diagram.png");
    assert_eq!(uploaded.file.tags, vec!["diagram", "image"]);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/files?scope=personal&tag=diagram&tag=image&limit=10")
                .body(Body::empty())
                .expect("list request should build"),
        )
        .await
        .expect("list request should complete");
    let listed: FileListResponse = response_success_json(response).await;
    assert_eq!(listed.data, vec![uploaded.file.clone()]);

    let file_url = format!("/v1/files/{}", uploaded.file.file_id);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&file_url)
                .body(Body::empty())
                .expect("read request should build"),
        )
        .await
        .expect("read request should complete");
    let read: FileReadResponse = response_success_json(response).await;
    assert_eq!(read.file, uploaded.file);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(&file_url)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"tags":[],"metadata":{"source":"updated"}}"#))
                .expect("update request should build"),
        )
        .await
        .expect("update request should complete");
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(&file_url)
                .body(Body::empty())
                .expect("delete request should build"),
        )
        .await
        .expect("delete request should complete");
    let deleted: FileDeleteResponse = response_success_json(response).await;
    assert_eq!(deleted.file_id, uploaded.file.file_id);

    let response = app
        .oneshot(
            Request::builder()
                .uri(&file_url)
                .body(Body::empty())
                .expect("read request should build"),
        )
        .await
        .expect("read request should complete");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn file_upload_rejects_transport_overflow_and_extra_parts() {
    let mut config = test_config();
    config.storage.files.limits.max_upload_bytes = 8;
    let app = file_router(test_state_with_config(config, false).await);

    let overflow = app
        .clone()
        .oneshot(multipart_request(
            upload_metadata(),
            b"\x89PNG\r\n\x1a\nX",
            false,
        ))
        .await
        .expect("overflow request should complete");
    assert_eq!(overflow.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let overflow_body: Value = response_json(overflow).await;
    assert_eq!(overflow_body["status"], "error");
    assert_eq!(overflow_body["error"]["code"], "uploadTooLarge");

    let extra = app
        .clone()
        .oneshot(multipart_request(
            upload_metadata(),
            b"\x89PNG\r\n\x1a\n",
            true,
        ))
        .await
        .expect("extra part request should complete");
    assert_eq!(extra.status(), StatusCode::BAD_REQUEST);

    let listed = app
        .oneshot(
            Request::builder()
                .uri("/v1/files")
                .body(Body::empty())
                .expect("list request should build"),
        )
        .await
        .expect("list request should complete");
    let listed: FileListResponse = response_success_json(listed).await;
    assert!(listed.data.is_empty());
}

#[tokio::test]
async fn file_upload_rejects_metadata_over_transport_limit() {
    let metadata = serde_json::json!({
        "filename": "diagram.png",
        "scope": { "type": "personal" },
        "tags": [],
        "metadata": { "payload": "x".repeat(64 * 1024) }
    });
    let response = file_router(test_state(false).await)
        .oneshot(multipart_request(metadata, b"\x89PNG\r\n\x1a\n", false))
        .await
        .expect("oversized metadata request should complete");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn file_list_query_rejects_project_id_without_project_scope() {
    let response = file_router(test_state(false).await)
        .oneshot(
            Request::builder()
                .uri("/v1/files?scope=personal&projectId=project-1")
                .body(Body::empty())
                .expect("list request should build"),
        )
        .await
        .expect("list request should complete");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn app_server_file_methods_use_the_same_file_service() {
    use talos_app_protocol::requests::{
        ClientMethod, ClientRequest, ClientRequestParams, FileUploadParams,
    };
    use talos_app_server::{AppServerApplication, TalosApplicationAdapter};
    use talos_types::{
        FileDeleteRequest, FileListRequest, FileReadRequest, FileScope, FileUpdateRequest,
        FileUploadRequest,
    };

    let state = test_state(false).await;
    let adapter = TalosApplicationAdapter::new(state.app.clone(), 32);
    let actor = talos_service::Actor::regular("static-token");
    let uploaded = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::FileUpload,
                params: ClientRequestParams::FileUpload(FileUploadParams {
                    request: FileUploadRequest {
                        filename: "diagram.png".to_string(),
                        scope: FileScope::Personal,
                        tags: vec!["diagram".to_string()],
                        metadata: Default::default(),
                    },
                    data_base64: "iVBORw0KGgo=".to_string(),
                }),
            },
        )
        .await
        .expect("app-server upload should succeed");
    let uploaded: FileUploadResponse =
        serde_json::from_value(uploaded).expect("upload response should decode");

    let listed = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::FileList,
                params: ClientRequestParams::FileList(FileListRequest::default()),
            },
        )
        .await
        .expect("app-server list should succeed");
    let listed: FileListResponse =
        serde_json::from_value(listed).expect("list response should decode");
    assert_eq!(listed.data, vec![uploaded.file.clone()]);

    adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::FileRead,
                params: ClientRequestParams::FileRead(FileReadRequest {
                    file_id: uploaded.file.file_id.clone(),
                }),
            },
        )
        .await
        .expect("app-server read should succeed");
    adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::FileUpdate,
                params: ClientRequestParams::FileUpdate(FileUpdateRequest {
                    file_id: uploaded.file.file_id.clone(),
                    tags: Some(Vec::new()),
                    metadata: None,
                }),
            },
        )
        .await
        .expect("app-server update should succeed");
    let deleted = adapter
        .dispatch(
            &actor,
            ClientRequest {
                method: ClientMethod::FileDelete,
                params: ClientRequestParams::FileDelete(FileDeleteRequest {
                    file_id: uploaded.file.file_id.clone(),
                }),
            },
        )
        .await
        .expect("app-server delete should succeed");
    let deleted: FileDeleteResponse =
        serde_json::from_value(deleted).expect("delete response should decode");
    assert_eq!(deleted.file_id, uploaded.file.file_id);
}

#[test]
fn file_error_mapping_is_exhaustive_and_machine_readable() {
    for (kind, status) in [
        (
            TalosFileErrorKind::UploadTooLarge,
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            TalosFileErrorKind::AttachmentLimitExceeded,
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            TalosFileErrorKind::UnsupportedMediaType,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ),
        (
            TalosFileErrorKind::UnsupportedAttachment,
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = ApiError::from_talos_error(TalosError::file_error(kind, "file rejected"))
            .into_response();
        assert_eq!(response.status(), status);
    }
}

fn upload_metadata() -> Value {
    serde_json::json!({
        "filename": "diagram.png",
        "scope": { "type": "personal" },
        "tags": [],
        "metadata": {}
    })
}

fn multipart_request(metadata: Value, file: &[u8], include_extra_part: bool) -> Request<Body> {
    const BOUNDARY: &str = "talos-file-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"metadata\"\r\n");
    body.extend_from_slice(b"Content-Type: application/json\r\n\r\n");
    body.extend_from_slice(metadata.to_string().as_bytes());
    body.extend_from_slice(format!("\r\n--{BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"ignored.bin\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(file);
    if include_extra_part {
        body.extend_from_slice(format!("\r\n--{BOUNDARY}\r\n").as_bytes());
        body.extend_from_slice(b"Content-Disposition: form-data; name=\"extra\"\r\n\r\nnope");
    }
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());

    Request::builder()
        .method("POST")
        .uri("/v1/files")
        .header(
            CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .expect("multipart request should build")
}

async fn response_json<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    serde_json::from_slice(&bytes).expect("response body should be valid JSON")
}

async fn response_success_json<T: serde::de::DeserializeOwned>(
    response: axum::response::Response,
) -> T {
    let envelope: talos_types::ApiResponse<T> = response_json(response).await;
    expect_success(envelope)
}
