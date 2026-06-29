use super::*;
use crate::runtimes::bash::{run_bash_command, BashCommandRequest, SandboxPermissions, TaskStatus};

#[tokio::test]
async fn test_task_output_tool_passes_offset_to_runtime() {
    let context = ToolContext::default()
        .with_project_root(std::env::current_dir().expect("cwd should exist"));
    let marker = "task-output-tool-offset-marker";
    let exec = run_bash_command(
        &BashCommandRequest {
            cmd: format!(
                "python3 -c \"import time; print('x' * 1300 + '{}', flush=True); time.sleep(1)\"",
                marker
            ),
            workdir: None,
            shell: Some("sh".to_string()),
            login: false,
            tty: false,
            yield_time_ms: 100,
            max_output_tokens: None,
            sandbox_permissions: SandboxPermissions::UseDefault,
            justification: None,
            prefix_rule: None,
        },
        &context,
    )
    .await
    .expect("bash command should start");

    assert_eq!(exec.status, TaskStatus::Running);
    let task_id = exec.session_id.expect("session id should be present");
    let payload = TaskOutputTool
        .execute_payload(
            serde_json::json!({
                "task_id": task_id,
                "offset": 1200,
                "max_output_tokens": 4096
            }),
            context,
        )
        .await
        .expect("TaskOutput should execute");

    let ToolOutputPayload::Function {
        structured_content: Some(content),
        content_items,
        ..
    } = payload
    else {
        panic!("expected structured TaskOutput payload");
    };
    let output = content
        .get("output")
        .and_then(Value::as_str)
        .expect("output should be a string");
    assert!(output.contains(marker), "{output}");
    assert!(
        !output.starts_with(&"x".repeat(1200)),
        "offset output should not include the skipped prefix"
    );

    let Some(TalosContentItem::BashOutput {
        stdout, truncated, ..
    }) = content_items.first()
    else {
        panic!("expected BashOutput content item");
    };
    assert!(stdout.contains(marker), "{stdout}");
    assert!(
        !stdout.starts_with(&"x".repeat(1200)),
        "typed output should preserve TaskOutput offset"
    );
    assert!(!truncated);
}
