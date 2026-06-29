use super::*;

#[tokio::test]
async fn grep_uses_rg_and_returns_search_matches() {
    let dir = tempfile::tempdir().expect("temp dir should create");
    let file = dir.path().join("demo.rs");
    tokio::fs::write(&file, "fn alpha() {}\nfn beta() {}\n")
        .await
        .expect("write should succeed");
    let context = crate::ToolContext::new("session", "user").with_project_root(dir.path());

    let result = run(
        &GrepFilesRequest {
            pattern: "alpha".to_string(),
            include: None,
            glob: Some("*.rs".to_string()),
            path: None,
            r#type: None,
            limit: 10,
            head_limit: None,
            offset: 0,
            output_mode: GrepOutputMode::Content,
            context: 0,
            before_context: None,
            after_context: None,
            multiline: false,
            case_insensitive: false,
            timeout_ms: 10_000,
        },
        &context,
    )
    .await
    .expect("grep should succeed");

    assert_eq!(result.total_matches, 1);
    assert_eq!(result.matches[0].line, 1);
    assert!(result.matches[0].text.contains("alpha"));
}
