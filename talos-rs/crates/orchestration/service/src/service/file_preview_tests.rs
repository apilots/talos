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

#[tokio::test]
async fn project_session_previews_files_only_inside_its_execution_worktree() {
    let checkout = tempfile::tempdir().unwrap();
    let checkout_root = checkout.path().canonicalize().unwrap();
    let worktree = checkout_root.join(".worktrees").join("feature");
    std::fs::create_dir_all(&worktree).unwrap();
    let plan = worktree.join("IMPLEMENTATION_PLAN.md");
    let markdown = b"# Worktree implementation plan";
    std::fs::write(&plan, markdown).unwrap();
    let outside = checkout_root.join("outside.md");
    std::fs::write(&outside, b"# Outside the worktree").unwrap();

    let (service, _) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("worktree-owner");
    let project = service
        .create_project(
            &actor,
            ProjectCreateRequest {
                node_id: None,
                name: "Feature worktree".into(),
                directory: worktree,
            },
        )
        .await
        .unwrap();
    let session_id = "worktree-preview";
    let mut params = create_session_params(session_id, request_source(), None);
    params.project_id = Some(project.project_id);
    service.create_session(&actor, params).await.unwrap();

    let record = service
        .access
        .load_session(&actor, session_id)
        .await
        .unwrap();
    let execution = service
        .sessions
        .build_agent_context(&record, AgentTurnRef::new(session_id, "turn-1"))
        .await
        .unwrap();
    assert_eq!(
        execution.working_directory.as_ref(),
        Some(&project.directory)
    );

    let prepare = |path: std::path::PathBuf| FilePreviewPrepareRequest {
        source: FilePreviewSource::SessionPath {
            session_id: session_id.into(),
            path: path.to_str().unwrap().into(),
        },
    };
    let preview = service
        .prepare_file_preview(&actor, prepare(plan))
        .await
        .unwrap();
    let content = service
        .read_file_preview(
            &actor,
            FilePreviewReadRequest {
                preview_id: preview.preview_id,
                resource_path: preview.entry_path,
                offset: 0,
                limit: 1024,
            },
        )
        .await
        .unwrap();
    assert_eq!(content.data.as_ref(), markdown);

    for path in [outside, "../../outside.md".into()] {
        let error = service
            .prepare_file_preview(&actor, prepare(path))
            .await
            .unwrap_err();
        assert!(error.is_code(talos_error::ErrorCode::PermissionDenied));
    }
}

fn git(directory: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(directory)
        .args([
            "-c",
            "user.name=Talos Test",
            "-c",
            "user.email=test@example.test",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn initialize_repository(path: &std::path::Path) {
    std::fs::create_dir(path).unwrap();
    git(
        path,
        &["init", "--quiet", "--template=", "--initial-branch=main"],
    );
    std::fs::write(path.join("plan.md"), "# Main checkout").unwrap();
    git(path, &["add", "plan.md"]);
    git(path, &["commit", "--quiet", "-m", "Initial"]);
}

fn session_path(session_id: &str, path: &std::path::Path) -> FilePreviewPrepareRequest {
    FilePreviewPrepareRequest {
        source: FilePreviewSource::SessionPath {
            session_id: session_id.into(),
            path: path.to_str().unwrap().into(),
        },
    }
}

async fn open_test_application(database: &std::path::Path) -> TalosService {
    let storage = Arc::new(
        SqliteStorage::new(database.to_str().unwrap())
            .await
            .unwrap(),
    );
    let (service, _) = test_application_with_storage(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        Arc::new(TestAgentCoordinator {
            cancel_mode: TestCancelMode::Cancelled,
            cancelled_turn_ids: Arc::new(Mutex::new(Vec::new())),
            compaction_usage: TokenUsage::default(),
            compaction_error: None,
        }),
        configured_test_config(),
        None,
        storage,
    )
    .await;
    service
}

#[tokio::test]
async fn conversation_worktree_binding_survives_reopen_and_scopes_preview() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let repository = base.join("repository");
    let checkout = base.join("linked");
    initialize_repository(&repository);
    git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            checkout.to_str().unwrap(),
            "HEAD",
        ],
    );
    std::fs::write(checkout.join("plan.md"), "# Worktree plan").unwrap();

    let database = base.join("sessions.sqlite");
    let service = open_test_application(&database).await;
    let actor = Actor::regular("worktree-owner");
    let project = service
        .create_project(
            &actor,
            ProjectCreateRequest {
                node_id: None,
                name: "Project".into(),
                directory: repository.clone(),
            },
        )
        .await
        .unwrap();
    let unrelated_repository = base.join("unrelated");
    let unrelated_worktree = base.join("unrelated-checkout");
    initialize_repository(&unrelated_repository);
    git(
        &unrelated_repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            unrelated_worktree.to_str().unwrap(),
            "HEAD",
        ],
    );
    let session_id = "bound-worktree";
    let mut params = create_session_params(session_id, request_source(), None);
    params.project_id = Some(project.project_id.clone());
    service.create_session(&actor, params).await.unwrap();
    let unrelated = service
        .start_turn(
            &actor,
            request_source(),
            start_turn_request(
                session_id,
                &format!("Bind existing worktree `{}`", unrelated_worktree.display()),
            ),
            None,
        )
        .await
        .unwrap_err();
    assert!(unrelated.is_code(talos_error::ErrorCode::PermissionDenied));
    let old_preview = service
        .prepare_file_preview(
            &actor,
            session_path(session_id, &repository.join("plan.md")),
        )
        .await
        .unwrap();

    service
        .start_turn(
            &actor,
            request_source(),
            start_turn_request(
                session_id,
                &format!("Bind existing worktree `{}`", checkout.display()),
            ),
            None,
        )
        .await
        .unwrap();
    let old_read = service
        .read_file_preview(
            &actor,
            FilePreviewReadRequest {
                preview_id: old_preview.preview_id,
                resource_path: old_preview.entry_path,
                offset: 0,
                limit: 1024,
            },
        )
        .await
        .unwrap_err();
    assert!(old_read.is_code(talos_error::ErrorCode::PermissionDenied));

    // Open a new Service with a separate connection to the same on-disk Session store.
    drop(service);
    let reopened = open_test_application(&database).await;
    let record = reopened
        .access
        .load_session(&actor, session_id)
        .await
        .unwrap();
    assert_eq!(
        record.project_id.as_deref(),
        Some(project.project_id.as_str())
    );
    assert_eq!(
        reopened
            .get_session(&actor, session_id)
            .await
            .unwrap()
            .current_directory
            .as_deref(),
        Some(checkout.as_path()),
    );
    let turn = AgentTurnRef::new(session_id, "resume");
    let context = reopened
        .sessions
        .build_agent_context(&record, turn.clone())
        .await
        .unwrap();
    assert_eq!(
        context.working_directory.as_deref(),
        Some(checkout.as_path())
    );

    let preview = reopened
        .prepare_file_preview(&actor, session_path(session_id, &checkout.join("plan.md")))
        .await
        .unwrap();
    let content = reopened
        .read_file_preview(
            &actor,
            FilePreviewReadRequest {
                preview_id: preview.preview_id,
                resource_path: preview.entry_path,
                offset: 0,
                limit: 1024,
            },
        )
        .await
        .unwrap();
    assert_eq!(content.data.as_ref(), b"# Worktree plan");
    #[cfg(unix)]
    std::os::unix::fs::symlink(repository.join("plan.md"), checkout.join("escape.md")).unwrap();
    for outside in [
        repository.join("plan.md"),
        std::path::PathBuf::from("../repository/plan.md"),
        #[cfg(unix)]
        checkout.join("escape.md"),
    ] {
        let error = reopened
            .prepare_file_preview(&actor, session_path(session_id, &outside))
            .await
            .unwrap_err();
        assert!(
            error.is_code(talos_error::ErrorCode::PermissionDenied),
            "{error:?}"
        );
    }

    git(
        &repository,
        &["worktree", "remove", "--force", checkout.to_str().unwrap()],
    );
    let execution_error = reopened
        .sessions
        .build_agent_context(&record, turn)
        .await
        .unwrap_err();
    let preview_error = reopened
        .prepare_file_preview(&actor, session_path(session_id, &checkout.join("plan.md")))
        .await
        .unwrap_err();
    for error in [execution_error, preview_error] {
        assert!(error.is_code(talos_error::ErrorCode::ResourceNotFound));
        assert!(error
            .to_api_error()
            .message
            .contains("Current working directory missing"));
    }
}

#[tokio::test]
async fn cli_conversation_creates_checkout_from_fetched_origin_main() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().canonicalize().unwrap();
    let remote = base.join("remote");
    let repository = base.join("repository");
    initialize_repository(&remote);
    git(
        &base,
        &[
            "clone",
            "--quiet",
            remote.to_str().unwrap(),
            repository.to_str().unwrap(),
        ],
    );
    std::fs::write(remote.join("plan.md"), "# Latest remote main").unwrap();
    git(
        &remote,
        &["commit", "--quiet", "-am", "Advance remote main"],
    );

    let (service, _) = test_application(
        Arc::new(TestAgentExecution {
            mode: TestAgentExecutionMode::Accepted,
        }),
        configured_test_config(),
    )
    .await;
    let actor = Actor::regular("worktree-owner");
    let session_id = "cli-worktree";
    let mut request = start_turn_request(
        session_id,
        "Create a new worktree from origin/main, then open plan.md.",
    );
    request.current_directory = Some(repository.clone());
    service
        .start_turn(
            &actor,
            SessionSource::new(SessionSourceType::Cli, SessionSourceProvider::Cli),
            request,
            None,
        )
        .await
        .unwrap();
    let record = service
        .access
        .load_session(&actor, session_id)
        .await
        .unwrap();
    let binding = record.worktree_binding.unwrap();
    assert!(binding.checkout_root.starts_with(base.join(".worktrees")));
    assert_eq!(
        git(&binding.checkout_root, &["rev-parse", "HEAD"]),
        git(&remote, &["rev-parse", "HEAD"])
    );
    assert_eq!(
        std::fs::read(repository.join("plan.md")).unwrap(),
        b"# Main checkout"
    );
}
