use super::*;

#[test]
fn apply_patch_reuses_its_parser_for_bounded_access_facts() {
    let dir = tempfile::tempdir().expect("temp dir should create");
    let source = dir.path().join("source.txt");
    let target = dir.path().join("target.txt");
    std::fs::write(&source, "alpha").expect("source should write");
    let context = ToolContext::new("session", "user").with_project_root(dir.path());

    let access = requested_file_system_access(
        &ApplyPatchRequest {
            patch: format!(
                "*** Begin Patch\n*** Update File: {}\n*** Move to: {}\n*** End Patch",
                source.display(),
                target.display()
            ),
        },
        &context,
    )
    .expect("patch access should resolve");

    assert_eq!(access.len(), 2);
    assert_eq!(access[0].operation, FileSystemOperation::Read);
    assert_eq!(access[0].paths[0].as_path(), source.canonicalize().unwrap());
    assert_eq!(access[1].operation, FileSystemOperation::Write);
    assert_eq!(
        access[1].paths[0].as_path(),
        dir.path().canonicalize().unwrap()
    );
}

#[tokio::test]
async fn apply_patch_adds_file() {
    let dir = tempfile::tempdir().expect("temp dir should create");
    let path = dir.path().join("demo.txt");
    let context = crate::test_support::full_access_file_context(
        ToolContext::new("session", "user").with_project_root(dir.path()),
    );

    let result = run(
        &ApplyPatchRequest {
            patch: format!(
                "*** Begin Patch\n*** Add File: {}\n+alpha\n+beta\n*** End Patch",
                path.display()
            ),
        },
        &context,
    )
    .await
    .expect("patch should apply");

    assert_eq!(result.changes[0].kind, ApplyPatchOperationKind::Add);
    assert_eq!(
        tokio::fs::read_to_string(path)
            .await
            .expect("file should exist"),
        "alpha\nbeta"
    );
}

#[tokio::test]
async fn apply_patch_accepts_trailing_newline_after_end_marker() {
    let dir = tempfile::tempdir().expect("temp dir should create");
    let path = dir.path().join("demo.txt");
    let context = crate::test_support::full_access_file_context(
        ToolContext::new("session", "user").with_project_root(dir.path()),
    );

    let result = run(
        &ApplyPatchRequest {
            patch: format!(
                "*** Begin Patch\n*** Add File: {}\n+alpha\n*** End Patch\n",
                path.display()
            ),
        },
        &context,
    )
    .await
    .expect("patch with trailing newline should apply");

    assert_eq!(result.changes[0].kind, ApplyPatchOperationKind::Add);
    assert_eq!(
        tokio::fs::read_to_string(path)
            .await
            .expect("file should exist"),
        "alpha"
    );
}

#[tokio::test]
async fn apply_patch_updates_file_with_read_state() {
    let file = tempfile::NamedTempFile::new().expect("temp file should create");
    tokio::fs::write(file.path(), "alpha\nbeta")
        .await
        .expect("write should succeed");
    let context = crate::test_support::full_access_file_context(
        ToolContext::new("session", "user")
            .with_project_root(file.path().parent().expect("parent should exist")),
    );
    context
        .read_file_state()
        .record_content(file.path().to_path_buf(), "alpha\nbeta");

    let result = run(
        &ApplyPatchRequest {
            patch: format!(
                "*** Begin Patch\n*** Update File: {}\n@@\n alpha\n-beta\n+gamma\n*** End Patch",
                file.path().display()
            ),
        },
        &context,
    )
    .await
    .expect("patch should apply");

    assert_eq!(result.changes[0].kind, ApplyPatchOperationKind::Update);
    assert_eq!(
        tokio::fs::read_to_string(file.path())
            .await
            .expect("file should read"),
        "alpha\ngamma"
    );
}

#[tokio::test]
async fn apply_patch_deletes_file_with_read_state() {
    let file = tempfile::NamedTempFile::new().expect("temp file should create");
    tokio::fs::write(file.path(), "alpha")
        .await
        .expect("write should succeed");
    let context = crate::test_support::full_access_file_context(
        ToolContext::new("session", "user")
            .with_project_root(file.path().parent().expect("parent should exist")),
    );
    context
        .read_file_state()
        .record_content(file.path().to_path_buf(), "alpha");

    run(
        &ApplyPatchRequest {
            patch: format!(
                "*** Begin Patch\n*** Delete File: {}\n*** End Patch",
                file.path().display()
            ),
        },
        &context,
    )
    .await
    .expect("patch should apply");

    assert!(!file.path().exists());
}

#[tokio::test]
async fn apply_patch_moves_file_with_read_state() {
    let dir = tempfile::tempdir().expect("temp dir should create");
    let source = dir.path().join("source.txt");
    let target = dir.path().join("target.txt");
    tokio::fs::write(&source, "alpha")
        .await
        .expect("write should succeed");
    let context = crate::test_support::full_access_file_context(
        ToolContext::new("session", "user").with_project_root(dir.path()),
    );
    context
        .read_file_state()
        .record_content(source.clone(), "alpha");

    let result = run(
        &ApplyPatchRequest {
            patch: format!(
                "*** Begin Patch\n*** Update File: {}\n*** Move to: {}\n*** End Patch",
                source.display(),
                target.display()
            ),
        },
        &context,
    )
    .await
    .expect("patch should apply");

    assert_eq!(result.changes[0].kind, ApplyPatchOperationKind::Move);
    assert!(!source.exists());
    assert_eq!(
        tokio::fs::read_to_string(target)
            .await
            .expect("target should read"),
        "alpha"
    );
}
