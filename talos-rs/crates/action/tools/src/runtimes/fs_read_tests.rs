use super::*;
use std::sync::Arc;
use tempfile::NamedTempFile;

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
    let context =
        crate::ToolContext::new_with_kernel_config("session", "user", Some(Arc::new(config)));

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
    let context = crate::ToolContext::new("session", "user");

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
    let context = crate::ToolContext::new("session", "user");

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
    let context = crate::ToolContext::new("session", "user");
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
