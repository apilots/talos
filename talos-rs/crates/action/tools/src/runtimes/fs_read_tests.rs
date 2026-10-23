use super::*;
use std::sync::Arc;
use tempfile::NamedTempFile;

#[tokio::test]
async fn different_line_ranges_do_not_return_an_unchanged_stub() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ranges.txt");
    std::fs::write(&path, "alpha\nbeta\ngamma\n").unwrap();
    let context = crate::test_support::full_access_file_context(
        crate::ToolContext::new("ranges", "user").with_project_root(root.path()),
    );
    let mut request: ReadFileRequest =
        serde_json::from_value(serde_json::json!({"path":path,"offset":1,"limit":1})).unwrap();
    assert_eq!(run(&request, &context).await.unwrap().content, "alpha");
    request.offset = 2;
    let next = run(&request, &context).await.unwrap();
    assert!(!next.unchanged);
    assert_eq!(next.content, "beta");
}

#[tokio::test]
async fn large_log_supports_line_reads_and_byte_continuation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("large.log");
    std::fs::write(&path, format!("{}END\n", "entry\n".repeat(400_000))).unwrap();
    let context = crate::test_support::full_access_file_context(
        crate::ToolContext::new("large-read", "user").with_project_root(root.path()),
    );
    let request: ReadFileRequest =
        serde_json::from_value(serde_json::json!({"path":path,"offset":400001,"limit":1})).unwrap();
    let result = run(&request, &context).await.unwrap();
    assert_eq!(result.content, "END");
    assert_eq!(result.total_lines, Some(400001));
    assert!(!result.truncated);
    let request: ReadFileRequest =
        serde_json::from_value(serde_json::json!({"path":path,"byte_offset":0,"max_bytes":12}))
            .unwrap();
    let result = run(&request, &context).await.unwrap();
    assert_eq!(result.content, "entry\nentry\n");
    assert_eq!(result.next_byte_offset, Some(12));
    assert!(result.total_lines.is_none());
}

#[tokio::test]
async fn byte_cursor_pages_preserve_utf8_without_skips_or_repetition() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("utf8.log");
    let text = "中文😀abc\n".repeat(20);
    std::fs::write(&path, &text).unwrap();
    let context = crate::test_support::full_access_file_context(
        crate::ToolContext::new("utf8-read", "user").with_project_root(root.path()),
    );
    let mut offset = 0;
    let mut combined = String::new();
    loop {
        let request: ReadFileRequest = serde_json::from_value(
            serde_json::json!({"path":path,"byte_offset":offset,"max_bytes":7}),
        )
        .unwrap();
        let result = run(&request, &context).await.unwrap();
        combined.push_str(&result.content);
        if let Some(next) = result.next_byte_offset {
            assert!(next > offset);
            offset = next;
        } else {
            break;
        }
    }
    assert_eq!(combined, text);
}

#[tokio::test]
async fn escaped_byte_ranges_fit_serialized_budget_before_advancing_the_cursor() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("escaped.log");
    std::fs::write(&path, "\u{1}\"\\\n".repeat(10_000)).unwrap();
    let context = crate::test_support::full_access_file_context(
        crate::ToolContext::new("escape-range", "user").with_project_root(root.path()),
    );
    let request: ReadFileRequest =
        serde_json::from_value(serde_json::json!({"path":path,"byte_offset":0,"max_bytes":8192}))
            .unwrap();
    let result = run(&request, &context).await.unwrap();
    assert!(serde_json::to_vec(&result).unwrap().len() < 10_000);
    assert_eq!(result.next_byte_offset, Some(result.content.len() as u64));
    assert!(!result.content.contains("[truncated]"));
}

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
            byte_offset: None,
            max_bytes: None,
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
            byte_offset: None,
            max_bytes: None,
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
            byte_offset: None,
            max_bytes: None,
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
            byte_offset: None,
            max_bytes: None,
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
        byte_offset: None,
        max_bytes: None,
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
