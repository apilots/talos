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

#[tokio::test]
async fn retained_project_metadata_and_deletion_do_not_require_its_worker_or_directory() {
    let (service, _) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::StartFailed,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("member-user");
    let directory = tempfile::tempdir().unwrap();
    for node in ["embedded", "offline-worker"] {
        // Persist the retained binding directly; no Worker is registered for offline-worker.
        let project = service
            .files
            .projects
            .create_project_on_node(
                "Retained project",
                &directory.path().join(node),
                &actor.user_id,
                node,
            )
            .await
            .unwrap();
        let other = Actor::regular("other-user");
        assert!(service
            .delete_project(&other, &project.project_id)
            .await
            .is_err());
        let info = service
            .get_project(&actor, &project.project_id)
            .await
            .unwrap();
        assert_eq!(info.directory, project.directory);
        let listed = service
            .list_projects(
                &actor,
                talos_types::ProjectListRequest {
                    node_id: Some(node.into()),
                },
            )
            .await
            .unwrap();
        assert_eq!(listed, vec![info]);
        service
            .delete_project(&actor, &project.project_id)
            .await
            .unwrap();
        assert!(service
            .files
            .projects
            .get_project(&project.project_id)
            .await
            .unwrap()
            .is_none());
    }
}

#[tokio::test]
async fn project_record_can_be_deleted_after_its_validated_directory_is_removed() {
    let (service, _) = test_application(
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
                name: "Removed directory".into(),
                directory: directory.path().to_path_buf(),
            },
        )
        .await
        .unwrap();
    directory.close().unwrap();
    service
        .delete_project(&actor, &project.project_id)
        .await
        .unwrap();
    assert!(service
        .files
        .projects
        .get_project(&project.project_id)
        .await
        .unwrap()
        .is_none());
}
