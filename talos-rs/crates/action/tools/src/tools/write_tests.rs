use super::*;
use talos_types::talos::TalosContentItem;

#[tokio::test]
async fn execute_payload_returns_diff_content_item() {
    let dir = tempfile::tempdir().expect("temp dir should create");
    let path = dir.path().join("demo.txt");
    let tool = WriteTool;

    let payload = tool
        .execute_payload(
            serde_json::json!({"path": path, "content": "alpha\n"}),
            ToolContext::new("session", "user"),
        )
        .await
        .expect("write should succeed");

    let ToolOutputPayload::Function { content_items, .. } = payload else {
        panic!("write should return function payload");
    };
    assert!(matches!(
        content_items.first(),
        Some(TalosContentItem::Diff { files }) if files.len() == 1
    ));
}
