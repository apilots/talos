use super::*;
use std::sync::Arc;
use tempfile::NamedTempFile;

#[test]
fn missing_path_retains_io_source_without_disclosing_absolute_path() {
    use std::error::Error;

    let workspace = tempfile::tempdir().unwrap();
    let path = workspace.path().join("private-path-sentinel");
    let error = normalize_existing_tool_path(&path).unwrap_err();
    assert_eq!(
        error
            .source()
            .and_then(|source| source.downcast_ref::<std::io::Error>())
            .map(std::io::Error::kind),
        Some(std::io::ErrorKind::NotFound)
    );
    assert!(!error
        .to_api_error()
        .message
        .contains("private-path-sentinel"));
    assert!(error.diagnostic_message().contains("private-path-sentinel"));
}

#[cfg(unix)]
#[test]
fn requested_read_access_resolves_a_symlink_to_its_target() {
    use std::os::unix::fs::symlink;

    let workspace = tempfile::tempdir().expect("workspace should create");
    let outside = NamedTempFile::new().expect("outside file should create");
    let link = workspace.path().join("outside-link");
    symlink(outside.path(), &link).expect("symlink should create");
    let context = crate::ToolContext::new("session", "user").with_project_root(workspace.path());

    let access = requested_file_system_access(
        &ReadFileRequest {
            path: Some(link.display().to_string()),
            file_path: None,
            offset: 1,
            limit: 10,
            pages: None,
        },
        &context,
    )
    .expect("read access should resolve");

    assert_eq!(
        access[0].paths[0].as_path(),
        outside.path().canonicalize().unwrap()
    );
}

#[tokio::test]
async fn test_read_file_runtime_reads_numbered_lines() {
    let file = NamedTempFile::new().expect("temp file should create");
    tokio::fs::write(file.path(), "alpha\nbeta\n")
        .await
        .expect("write should succeed");

    let config = talos_config::AgentKernelConfig {
        workspace_dir: file
            .path()
            .parent()
            .expect("parent should exist")
            .to_string_lossy()
            .to_string(),
        ..Default::default()
    };
    let context = crate::test_support::full_access_file_context(
        crate::ToolContext::new_with_kernel_config("session", "user", Some(Arc::new(config)))
            .with_project_root(file.path().parent().expect("parent should exist")),
    );

    let output = run(
        &ReadFileRequest {
            path: Some(file.path().to_string_lossy().to_string()),
            file_path: None,
            offset: 1,
            limit: 2,
            pages: None,
        },
        &context,
    )
    .await
    .expect("read should succeed");

    assert_eq!(output.content, "alpha\nbeta");
    assert_eq!(output.offset, 0);
    assert!(context.read_file_state().contains(file.path()));
}

#[tokio::test]
async fn test_read_file_runtime_reports_zero_based_offset() {
    let file = NamedTempFile::new().expect("temp file should create");
    tokio::fs::write(file.path(), "alpha\nbeta\ngamma\n")
        .await
        .expect("write should succeed");
    let context = crate::test_support::full_access_file_context(
        crate::ToolContext::new("session", "user")
            .with_project_root(file.path().parent().expect("parent should exist")),
    );

    let output = run(
        &ReadFileRequest {
            path: Some(file.path().to_string_lossy().to_string()),
            file_path: None,
            offset: 2,
            limit: 1,
            pages: None,
        },
        &context,
    )
    .await
    .expect("read should succeed");

    assert_eq!(output.offset, 1);
    assert_eq!(output.content, "beta");
    assert!(!output.content.contains("L2:"));
}

#[tokio::test]
async fn test_read_file_runtime_rejects_directories() {
    let dir = tempfile::tempdir().expect("temp dir should create");
    let context = crate::test_support::full_access_file_context(
        crate::ToolContext::new("session", "user").with_project_root(dir.path()),
    );

    let error = run(
        &ReadFileRequest {
            path: Some(dir.path().to_string_lossy().to_string()),
            file_path: None,
            offset: 1,
            limit: 2,
            pages: None,
        },
        &context,
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("directory"));
}

#[tokio::test]
async fn test_read_file_runtime_returns_unchanged_stub_for_repeat_read() {
    let file = NamedTempFile::new().expect("temp file should create");
    tokio::fs::write(file.path(), "alpha\n")
        .await
        .expect("write should succeed");
    let context = crate::test_support::full_access_file_context(
        crate::ToolContext::new("session", "user")
            .with_project_root(file.path().parent().expect("parent should exist")),
    );
    let request = ReadFileRequest {
        path: Some(file.path().to_string_lossy().to_string()),
        file_path: None,
        offset: 1,
        limit: 2,
        pages: None,
    };

    run(&request, &context)
        .await
        .expect("first read should succeed");
    let output = run(&request, &context)
        .await
        .expect("second read should succeed");

    assert!(output.unchanged);
    assert!(output.content.contains("File unchanged"));
}
