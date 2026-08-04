use super::*;

#[test]
fn validate_input_rejects_missing_path() {
    let tool = ReadTool;
    let error = tool
        .validate_input(&serde_json::json!({"offset": 1, "limit": 10}))
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
}

#[tokio::test]
async fn execute_payload_returns_file_read_content_item() {
    let file = tempfile::NamedTempFile::new().expect("temp file should create");
    tokio::fs::write(file.path(), "alpha\n")
        .await
        .expect("write should succeed");
    let tool = ReadTool;
    let payload = tool
        .execute_payload(
            serde_json::json!({"path": file.path(), "offset": 1, "limit": 10}),
            ToolContext::new("session", "user"),
        )
        .await
        .expect("read should succeed");

    let ToolOutputPayload::Function { content_items, .. } = payload else {
        panic!("read should return function payload");
    };
    assert!(matches!(
        content_items.first(),
        Some(TalosContentItem::FileRead { content, .. }) if content == "alpha"
    ));
}
