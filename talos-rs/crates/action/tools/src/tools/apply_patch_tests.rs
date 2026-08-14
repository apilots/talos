use super::*;

#[test]
fn validate_input_rejects_missing_patch() {
    let tool = ApplyPatchTool;
    let error = tool.validate_input(&serde_json::json!({})).unwrap_err();

    assert!(matches!(error, TalosError::ValidationError(_)));
}

#[tokio::test]
async fn execute_payload_returns_diff_content_item() {
    let dir = tempfile::tempdir().expect("temp dir should create");
    let path = dir.path().join("demo.txt");
    let tool = ApplyPatchTool;

    let payload = tool
        .execute_payload(
            serde_json::json!({
                "patch": format!(
                    "*** Begin Patch\n*** Add File: {}\n+alpha\n*** End Patch",
                    path.display()
                )
            }),
            crate::test_support::full_access_file_context(
                ToolContext::new("session", "user").with_project_root(dir.path()),
            ),
        )
        .await
        .expect("patch should apply");

    let ToolOutputPayload::Function { content_items, .. } = payload else {
        panic!("apply patch should return function payload");
    };
    assert!(matches!(
        content_items.first(),
        Some(TalosContentItem::Diff { files }) if files.len() == 1
    ));
}
