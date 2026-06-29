use super::*;

#[tokio::test]
async fn execute_payload_returns_search_results_content_item() {
    let dir = tempfile::tempdir().expect("temp dir should create");
    tokio::fs::write(dir.path().join("demo.rs"), "fn alpha() {}\n")
        .await
        .expect("write should succeed");
    let context = ToolContext::new("session", "user").with_project_root(dir.path());

    let payload = GrepTool
        .execute_payload(
            serde_json::json!({
                "pattern": "alpha",
                "glob": "*.rs",
                "output_mode": "content"
            }),
            context,
        )
        .await
        .expect("grep should execute");

    let ToolOutputPayload::Function { content_items, .. } = payload else {
        panic!("grep should return function payload");
    };
    assert!(matches!(
        content_items.first(),
        Some(TalosContentItem::SearchResults { matches, .. }) if matches.len() == 1
    ));
}
