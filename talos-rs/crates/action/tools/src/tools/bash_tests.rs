use super::*;

#[test]
fn validate_input_rejects_missing_command() {
    let tool = BashTool;
    let error = tool
        .validate_input(&serde_json::json!({"timeout": 10}))
        .unwrap_err();

    assert!(matches!(error, TalosError::ValidationError(_)));
}

#[test]
fn check_permissions_rejects_sandbox_escalation() {
    let tool = BashTool;
    let context = ToolContext::default();
    let input = serde_json::json!({
        "command": "echo hi",
        "dangerously_disable_sandbox": true
    });

    let error = tool.check_permissions(&input, &context).unwrap_err();
    assert!(matches!(error, TalosError::AuthorizationDenied));
}

#[tokio::test]
async fn execute_payload_uses_truncated_output_chunk_for_typed_result() {
    let tool = BashTool;
    let context = ToolContext::default()
        .with_project_root(std::env::current_dir().expect("cwd should exist"));
    let payload = tool
        .execute_payload(
            serde_json::json!({
                "command": "python3 -c \"print('x' * 20000)\""
            }),
            context,
        )
        .await
        .expect("Bash should execute");

    let ToolOutputPayload::Function { content_items, .. } = payload else {
        panic!("expected function payload");
    };
    let Some(TalosContentItem::BashOutput {
        stdout,
        stderr,
        truncated,
        ..
    }) = content_items.first()
    else {
        panic!("expected BashOutput content item");
    };

    assert!(stderr.is_empty());
    assert!(*truncated);
    assert!(
        stdout.len() < 20_000,
        "typed output should use the returned chunk, not cumulative stdout"
    );
}
