use super::*;
use talos_types::*;

#[tokio::test]
async fn registered_preview_keeps_actor_ownership_ranges_and_close_lifecycle() {
    let mut config = TalosConfig::default();
    config.storage.files.preview_base_url = Some("https://preview.example.com/".into());
    let (service, _) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        config,
    )
    .await;
    let actor = crate::Actor::regular("preview-owner");
    let other = crate::Actor::regular("other-owner");
    let bytes = Bytes::from_static(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
    let uploaded = service
        .files
        .upload(
            &actor,
            FileUploadRequest {
                filename: "picture.png".into(),
                scope: FileScope::Personal,
                tags: vec![],
                metadata: BTreeMap::new(),
            },
            Box::pin(stream::iter([Ok(bytes.clone())])),
        )
        .await
        .unwrap()
        .file;
    let prepared = service
        .prepare_file_preview(
            &actor,
            FilePreviewPrepareRequest {
                source: FilePreviewSource::File {
                    file_id: uploaded.file_id,
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(
        prepared.resource_base_url,
        Some(format!(
            "https://{}.preview.example.com/",
            prepared.preview_id
        ))
    );
    let read = FilePreviewReadRequest {
        preview_id: prepared.preview_id.clone(),
        resource_path: prepared.entry_path.clone(),
        offset: 1,
        limit: 3,
    };
    let content = service
        .read_file_preview(&actor, read.clone())
        .await
        .unwrap();
    assert_eq!(content.data.as_ref(), b"PNG");
    assert!(service
        .read_file_preview(&other, read.clone())
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::ResourceNotFound));
    assert!(service
        .close_file_preview(
            &other,
            FilePreviewCloseRequest {
                preview_id: prepared.preview_id.clone()
            }
        )
        .await
        .is_err());
    assert!(
        service
            .close_file_preview(
                &actor,
                FilePreviewCloseRequest {
                    preview_id: prepared.preview_id
                }
            )
            .await
            .unwrap()
            .closed
    );
    assert!(service
        .read_file_preview_capability(read)
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::ResourceNotFound));
}

#[tokio::test]
async fn session_preview_uses_saved_directory_and_rechecks_session_access() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("report.html"), b"<p>session</p>").unwrap();
    let (service, _) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config(),
    )
    .await;
    let actor = crate::Actor::regular("preview-owner");
    service
        .create_session(
            &actor,
            create_session_params(
                "preview-session",
                request_source(),
                Some(temp.path().to_owned()),
            ),
        )
        .await
        .unwrap();
    let prepared = service
        .prepare_file_preview(
            &actor,
            FilePreviewPrepareRequest {
                source: FilePreviewSource::SessionPath {
                    session_id: "preview-session".into(),
                    path: "report.html".into(),
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(prepared.metadata.name, "report.html");
    let request = FilePreviewReadRequest {
        preview_id: prepared.preview_id.clone(),
        resource_path: prepared.entry_path,
        offset: 3,
        limit: 7,
    };
    let part = service
        .read_file_preview(&actor, request.clone())
        .await
        .unwrap();
    assert_eq!(part.data.as_ref(), b"session");
    service
        .delete_session(&actor, "preview-session")
        .await
        .unwrap();
    assert!(service.read_file_preview(&actor, request).await.is_err());
    service
        .close_file_preview(
            &actor,
            FilePreviewCloseRequest {
                preview_id: prepared.preview_id,
            },
        )
        .await
        .unwrap();
}
