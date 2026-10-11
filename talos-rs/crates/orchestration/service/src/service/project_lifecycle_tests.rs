use super::*;

#[tokio::test]
async fn project_delete_is_blocked_until_owned_files_are_removed() {
    let (service, _store) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    let directory = tempfile::tempdir().unwrap();
    let project = service
        .create_project(
            &actor,
            talos_types::ProjectCreateRequest {
                node_id: None,
                name: "Project with file".to_string(),
                directory: directory.path().to_path_buf(),
            },
        )
        .await
        .unwrap();
    let uploaded = service
        .files
        .upload(
            &actor,
            talos_types::FileUploadRequest {
                filename: "image.png".to_string(),
                scope: talos_types::FileScope::Project {
                    project_id: project.project_id.clone(),
                },
                tags: Vec::new(),
                metadata: BTreeMap::new(),
            },
            Box::pin(stream::iter([Ok(Bytes::from_static(
                b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR",
            ))])),
        )
        .await
        .unwrap();

    let conflict = service
        .delete_project(&actor, &project.project_id)
        .await
        .unwrap_err();
    assert!(conflict.is_code(talos_error::ErrorCode::Conflict));

    service
        .files
        .delete(
            &actor,
            talos_types::FileDeleteRequest {
                file_id: uploaded.file.file_id,
            },
        )
        .await
        .unwrap();
    service
        .delete_project(&actor, &project.project_id)
        .await
        .unwrap();
}
