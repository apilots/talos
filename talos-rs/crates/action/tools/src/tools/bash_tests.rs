use super::*;

#[test]
fn validate_input_rejects_missing_command() {
    let tool = BashTool;
    let error = tool
        .validate_input(&serde_json::json!({"timeout": 10}))
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
}

#[tokio::test]
async fn execute_payload_uses_truncated_output_chunk_for_typed_result() {
    let tool = BashTool;
    let context = crate::test_support::full_access_process_context(
        ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist")),
    );
    let payload = tool
        .execute_payload(
            serde_json::json!({
                "command": "python3 -c \"print('x' * 20000)\""
            }),
            context,
        )
        .await
        .expect("Bash should execute");
    assert!(!payload.is_error());

    let ToolOutputPayload::Function { content_items, .. } = payload else {
        panic!("expected function payload");
    };
    let Some(TalosContentItem::BashOutput {
        session_id,
        stdout,
        stderr,
        truncated,
        ..
    }) = content_items.first()
    else {
        panic!("expected BashOutput content item");
    };

    assert!(stderr.is_empty());
    assert_eq!(*session_id, None);
    assert!(*truncated);
    assert!(
        stdout.len() < 20_000,
        "typed output should use the returned chunk, not cumulative stdout"
    );
}

#[tokio::test]
async fn execute_payload_returns_timeout_without_running_session() {
    let tool = BashTool;
    let context = crate::test_support::full_access_process_context(
        ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist")),
    );
    let payload = tool
        .execute_payload(
            serde_json::json!({
                "command": "sleep 1",
                "timeout": 50
            }),
            context,
        )
        .await
        .expect("Bash should return timeout payload");
    assert!(payload.is_error());

    let ToolOutputPayload::Function { content_items, .. } = payload else {
        panic!("expected function payload");
    };
    let Some(TalosContentItem::BashOutput {
        session_id,
        status,
        exit_code,
        ..
    }) = content_items.first()
    else {
        panic!("expected BashOutput content item");
    };

    assert_eq!(*session_id, None);
    assert_eq!(*status, TalosToolTaskStatus::TimedOut);
    assert_eq!(*exit_code, Some(124));
}

#[tokio::test]
async fn execute_payload_marks_non_zero_exit_as_failure_and_preserves_bash_output() {
    let tool = BashTool;
    let context = crate::test_support::full_access_process_context(
        ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist")),
    );
    let payload = tool
        .execute_payload(
            serde_json::json!({
                "command": "printf failure-output; exit 69"
            }),
            context,
        )
        .await
        .expect("Bash should return a failed payload");

    assert!(payload.is_error());
    let ToolOutputPayload::Function {
        success,
        content_items,
        ..
    } = payload
    else {
        panic!("expected function payload");
    };
    assert!(!success);
    assert!(matches!(
        content_items.as_slice(),
        [TalosContentItem::BashOutput {
            status: TalosToolTaskStatus::Failed,
            exit_code: Some(69),
            stdout,
            ..
        }] if stdout == "failure-output"
    ));
}
