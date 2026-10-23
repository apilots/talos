use super::*;

#[tokio::test]
async fn insufficient_result_budget_is_rejected_before_starting_the_command() {
    let workspace = tempfile::tempdir().unwrap();
    let config = talos_config::AgentKernelConfig {
        workspace_dir: workspace.path().to_string_lossy().into_owned(),
        tool_result_max_bytes: 3000,
        ..Default::default()
    };
    let mut context = crate::test_support::full_access_process_context(
        ToolContext::new_with_kernel_config("session", "user", Some(std::sync::Arc::new(config)))
            .with_turn_id("turn")
            .with_project_root(workspace.path()),
    );
    context.bind_current_call_id("too-small");
    let error = BashTool
        .execute_payload(
            serde_json::json!({
                "command": "touch must-not-execute"
            }),
            context,
        )
        .await
        .unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
    assert!(!workspace.path().join("must-not-execute").exists());
}

#[tokio::test]
async fn failed_multi_megabyte_output_is_saved_and_only_the_tail_is_returned() {
    let workspace = tempfile::tempdir().unwrap();
    let config = talos_config::AgentKernelConfig {
        workspace_dir: workspace.path().to_string_lossy().into_owned(),
        ..Default::default()
    };
    let mut context = crate::test_support::full_access_process_context(
        ToolContext::new_with_kernel_config("session", "user", Some(std::sync::Arc::new(config)))
            .with_turn_id("turn")
            .with_project_root(workspace.path()),
    );
    context.bind_current_call_id("large-failure");
    let payload = BashTool.execute_payload(serde_json::json!({
        "command": "python3 -c \"import sys; sys.stdout.write('START\\n' + 'log line\\n' * 350000 + 'FINAL ERROR\\n'); sys.exit(7)\""
    }), context).await.unwrap();
    assert!(payload.is_error());
    let envelope = payload.envelope();
    assert!(
        envelope.error.as_ref().unwrap().message.len() <= 200,
        "a failed command must not use the entire response JSON as its error"
    );
    let ToolOutputPayload::Function {
        content_items,
        structured_content,
        ..
    } = payload
    else {
        panic!("function result")
    };
    let TalosContentItem::BashOutput {
        stdout,
        stderr,
        output_log: Some(log),
        truncated,
        exit_code,
        ..
    } = &content_items[0]
    else {
        panic!("saved Bash output")
    };
    assert_eq!(*exit_code, Some(7));
    assert!(*truncated);
    assert!(stdout.contains("FINAL ERROR"));
    assert!(stdout.lines().count() + stderr.lines().count() <= 60);
    assert_eq!(
        log.capture_status,
        talos_types::talos::BashOutputCaptureStatus::Complete
    );
    let full = std::fs::read_to_string(&log.path).unwrap();
    assert!(full.len() > 2 * 1024 * 1024);
    assert!(full.starts_with("START\n"));
    assert!(full.ends_with("FINAL ERROR\n"));
    assert_eq!(log.total_bytes, full.len() as u64);
    assert!(serde_json::to_vec(&structured_content).unwrap().len() < 32 * 1024);
}

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
    let workspace = tempfile::tempdir().unwrap();
    let config = talos_config::AgentKernelConfig {
        workspace_dir: workspace.path().to_string_lossy().into_owned(),
        ..Default::default()
    };
    let mut context = crate::test_support::full_access_process_context(
        ToolContext::new_with_kernel_config(
            "test-session",
            "test-user",
            Some(std::sync::Arc::new(config)),
        )
        .with_turn_id("test-turn")
        .with_project_root(std::env::current_dir().expect("cwd should exist")),
    );
    context.bind_current_call_id("test-bash-call");
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
    let workspace = tempfile::tempdir().unwrap();
    let config = talos_config::AgentKernelConfig {
        workspace_dir: workspace.path().to_string_lossy().into_owned(),
        ..Default::default()
    };
    let mut context = crate::test_support::full_access_process_context(
        ToolContext::new_with_kernel_config(
            "test-session",
            "test-user",
            Some(std::sync::Arc::new(config)),
        )
        .with_turn_id("test-turn")
        .with_project_root(std::env::current_dir().expect("cwd should exist")),
    );
    context.bind_current_call_id("test-bash-call");
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
    let workspace = tempfile::tempdir().unwrap();
    let config = talos_config::AgentKernelConfig {
        workspace_dir: workspace.path().to_string_lossy().into_owned(),
        ..Default::default()
    };
    let mut context = crate::test_support::full_access_process_context(
        ToolContext::new_with_kernel_config(
            "test-session",
            "test-user",
            Some(std::sync::Arc::new(config)),
        )
        .with_turn_id("test-turn")
        .with_project_root(std::env::current_dir().expect("cwd should exist")),
    );
    context.bind_current_call_id("test-bash-call");
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
