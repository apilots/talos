use super::*;

#[tokio::test]
async fn glob_returns_sorted_paginated_search_results() {
    let dir = tempfile::tempdir().expect("temp dir should create");
    tokio::fs::write(dir.path().join("b.rs"), "")
        .await
        .expect("write should succeed");
    tokio::fs::write(dir.path().join("a.rs"), "")
        .await
        .expect("write should succeed");
    let context = crate::test_support::full_access_file_context(
        crate::ToolContext::new("session", "user").with_project_root(dir.path()),
    );

    let result = run(
        &GlobFilesRequest {
            pattern: "*.rs".to_string(),
            path: None,
            limit: Some(1),
            offset: Some(1),
        },
        &context,
    )
    .await
    .expect("glob should succeed");

    assert_eq!(result.total_matches, 2);
    assert_eq!(result.matches[0].path, "b.rs");
    assert!(!result.more_available);
}
