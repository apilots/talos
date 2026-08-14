use super::*;
use talos_types::talos::TalosContentItem;

#[test]
fn validate_input_rejects_missing_required_fields() {
    let tool = EditTool;
    let error = tool
        .validate_input(&serde_json::json!({"path": "demo.txt"}))
        .unwrap_err();

    assert!(matches!(error, TalosError::ValidationError(_)));
}

#[tokio::test]
async fn execute_payload_returns_diff_content_item() {
    let file = tempfile::NamedTempFile::new().expect("temp file should create");
    tokio::fs::write(file.path(), "alpha\n")
        .await
        .expect("write should succeed");
    let context = crate::test_support::full_access_file_context(
        ToolContext::new("session", "user")
            .with_project_root(file.path().parent().expect("parent should exist")),
    );
    context
        .read_file_state()
        .record_content(file.path().to_path_buf(), "alpha\n");
    let tool = EditTool;

    let payload = tool
        .execute_payload(
            serde_json::json!({
                "path": file.path(),
                "old_string": "alpha",
                "new_string": "beta"
            }),
            context,
        )
        .await
        .expect("edit should succeed");

    let ToolOutputPayload::Function { content_items, .. } = payload else {
        panic!("edit should return function payload");
    };
    assert!(matches!(
        content_items.first(),
        Some(TalosContentItem::Diff { files }) if files.len() == 1
    ));
}
