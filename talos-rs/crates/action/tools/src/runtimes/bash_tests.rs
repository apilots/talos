use super::*;

#[tokio::test]
async fn test_bash_runtime_supports_sessionized_task_output() {
    let context = crate::ToolContext::default()
        .with_project_root(std::env::current_dir().expect("cwd should exist"));
    let exec = run_bash_command(
        &BashCommandRequest {
            cmd: "printf 'ready\\n'; read line; printf 'done:%s\\n' \"$line\"".to_string(),
            workdir: None,
            shell: Some("sh".to_string()),
            login: false,
            tty: false,
            yield_time_ms: 50,
            max_output_tokens: None,
            sandbox_permissions: SandboxPermissions::UseDefault,
            justification: None,
            prefix_rule: None,
        },
        &context,
    )
    .await
    .expect("bash command should succeed");

    assert_eq!(exec.status, TaskStatus::Running);
    assert!(exec.output.contains("ready"));

    let session_id = exec.session_id.expect("session id should be present");
    let output = read_task_output_with_context(
        &TaskOutputRequest {
            session_id,
            chars: "ok\n".to_string(),
            yield_time_ms: 50,
            offset: None,
            max_output_tokens: None,
        },
        &context,
    )
    .await
    .expect("task output read should succeed");

    assert!(output.output.contains("done:ok"));
    assert_eq!(output.status, TaskStatus::Exited);
}

#[tokio::test]
async fn test_task_output_reads_from_explicit_offset() {
    let context = crate::ToolContext::default()
        .with_project_root(std::env::current_dir().expect("cwd should exist"));
    let marker = "task-output-offset-marker";
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

    let session_id = exec.session_id.expect("session id should be present");
    let output = read_task_output_with_context(
        &TaskOutputRequest {
            session_id,
            chars: String::new(),
            yield_time_ms: 1_000,
            offset: Some(1200),
            max_output_tokens: None,
        },
        &context,
    )
    .await
    .expect("task output read should succeed");

    assert!(output.output.contains(marker), "{}", output.output);
    assert!(
        !output.output.starts_with(&"x".repeat(1200)),
        "offset output should not include the skipped prefix"
    );
}

#[tokio::test]
async fn test_bash_runtime_allows_escalated_command_when_called_directly() {
    let context = crate::ToolContext::default()
        .with_project_root(std::env::current_dir().expect("cwd should exist"));
    let result = run_bash_command(
        &BashCommandRequest {
            cmd: "echo hi".to_string(),
            workdir: None,
            shell: Some("sh".to_string()),
            login: false,
            tty: false,
            yield_time_ms: 50,
            max_output_tokens: None,
            sandbox_permissions: SandboxPermissions::RequireEscalated,
            justification: Some("needs unrestricted filesystem access".to_string()),
            prefix_rule: None,
        },
        &context,
    )
    .await
    .expect("direct runtime execution should allow explicit sandbox escalation");

    assert!(result.output.contains("hi") || result.status == TaskStatus::Exited);
}

#[tokio::test]
async fn test_bash_runtime_separates_stdout_stderr_and_classifies_command() {
    let context = crate::ToolContext::default()
        .with_project_root(std::env::current_dir().expect("cwd should exist"));
    let result = run_bash_command(
        &BashCommandRequest {
            cmd: "printf out; printf err >&2".to_string(),
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
    .expect("bash command should succeed");

    assert!(result.stdout.contains("out"));
    assert!(result.stderr.contains("err"));
    assert!(result.elapsed_ms <= 10_000);

    let classified = run_bash_command(
        &BashCommandRequest {
            cmd: "git status --short".to_string(),
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
    .expect("bash command should succeed");

    assert_eq!(classified.command_kind, Some(BashCommandKind::ReadOnly));
    assert_eq!(classified.risk, Some(BashCommandRisk::Low));
}

#[tokio::test]
async fn test_task_stop_terminates_running_process() {
    let context = crate::ToolContext::new("stop-session", "user")
        .with_project_root(std::env::current_dir().expect("cwd should exist"));
    let exec = run_bash_command(
        &BashCommandRequest {
            cmd: "sleep 5".to_string(),
            workdir: None,
            shell: Some("sh".to_string()),
            login: false,
            tty: false,
            yield_time_ms: 20,
            max_output_tokens: None,
            sandbox_permissions: SandboxPermissions::UseDefault,
            justification: None,
            prefix_rule: None,
        },
        &context,
    )
    .await
    .expect("bash command should start");

    let session_id = exec.session_id.expect("session id should be present");
    let stopped = stop_task_with_context(session_id, &context)
        .await
        .expect("task stop should succeed");

    assert_eq!(stopped.status, TaskStatus::Stopped);
    assert_eq!(stopped.exit_code, Some(130));
}

#[tokio::test]
async fn test_task_output_rejects_cross_session_access() {
    let owner_context = crate::ToolContext::new("session-owner", "user-1")
        .with_project_root(std::env::current_dir().expect("cwd should exist"));
    let foreign_context = crate::ToolContext::new("session-foreign", "user-2")
        .with_project_root(std::env::current_dir().expect("cwd should exist"));
    let exec = run_bash_command(
        &BashCommandRequest {
            cmd: "printf 'ready\\n'; sleep 1".to_string(),
            workdir: None,
            shell: Some("sh".to_string()),
            login: false,
            tty: false,
            yield_time_ms: 50,
            max_output_tokens: None,
            sandbox_permissions: SandboxPermissions::UseDefault,
            justification: None,
            prefix_rule: None,
        },
        &owner_context,
    )
    .await
    .expect("bash command should succeed");

    let session_id = exec.session_id.expect("session id should be present");
    let error = read_task_output_with_context(
        &TaskOutputRequest {
            session_id,
            chars: "noop".to_string(),
            yield_time_ms: 0,
            offset: None,
            max_output_tokens: None,
        },
        &foreign_context,
    )
    .await
    .expect_err("cross-session task output read should be rejected");

    assert!(error
        .to_string()
        .contains("does not belong to Talos session session-foreign"));
}
