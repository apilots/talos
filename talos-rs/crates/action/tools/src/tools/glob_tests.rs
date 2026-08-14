use super::*;

#[tokio::test]
async fn execute_payload_returns_search_results_content_item() {
    let dir = tempfile::tempdir().expect("temp dir should create");
    tokio::fs::write(dir.path().join("demo.rs"), "")
        .await
        .expect("write should succeed");
    let context = crate::test_support::full_access_file_context(
        ToolContext::new("session", "user").with_project_root(dir.path()),
    );

    let payload = GlobTool
        .execute_payload(serde_json::json!({"pattern": "*.rs"}), context)
        .await
        .expect("glob should execute");

    let ToolOutputPayload::Function { content_items, .. } = payload else {
        panic!("glob should return function payload");
    };
    assert!(matches!(
        content_items.first(),
        Some(TalosContentItem::SearchResults { matches, .. }) if matches.len() == 1
    ));
}
