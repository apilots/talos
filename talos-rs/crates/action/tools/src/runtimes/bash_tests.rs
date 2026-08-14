use super::*;

#[tokio::test]
async fn test_short_bash_command_exits_without_running_session() {
    let context = crate::test_support::full_access_process_context(
        crate::ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist")),
    );
    let result = run_bash_command(
        &BashCommandRequest {
            cmd: "printf 'done\\n'".to_string(),
            workdir: None,
            shell: Some("sh".to_string()),
            login: false,
            tty: false,
            yield_time_ms: 1_000,
            timeout_ms: Some(5_000),
            max_output_tokens: None,
            sandbox_permissions: SandboxPermissions::UseDefault,
            justification: None,
            prefix_rule: None,
        },
        &context,
    )
    .await
    .expect("bash command should succeed");

    assert_eq!(result.status, BashRunStatus::Exited);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.session_id, None);
    assert!(result.output.contains("done"));
}

#[tokio::test]
async fn test_completed_truncated_bash_output_does_not_keep_session() {
    let context = crate::test_support::full_access_process_context(
        crate::ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist")),
    );
    let result = run_bash_command(
        &BashCommandRequest {
            cmd: "python3 -c \"print('x' * 20000)\"".to_string(),
            workdir: None,
            shell: Some("sh".to_string()),
            login: false,
            tty: false,
            yield_time_ms: 1_000,
            timeout_ms: Some(5_000),
            max_output_tokens: Some(100),
            sandbox_permissions: SandboxPermissions::UseDefault,
            justification: None,
            prefix_rule: None,
        },
        &context,
    )
    .await
    .expect("bash command should succeed");

    assert_eq!(result.status, BashRunStatus::Exited);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.session_id, None);
    assert!(result.truncated);
    assert!(result.output.len() < 20_000);
}

#[tokio::test]
async fn test_bash_command_uses_own_timeout() {
    let context = crate::test_support::full_access_process_context(
        crate::ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist")),
    );
    let result = run_bash_command(
        &BashCommandRequest {
            cmd: "sleep 5".to_string(),
            workdir: None,
            shell: Some("sh".to_string()),
            login: false,
            tty: false,
            yield_time_ms: 5_000,
            timeout_ms: Some(50),
            max_output_tokens: None,
            sandbox_permissions: SandboxPermissions::UseDefault,
            justification: None,
            prefix_rule: None,
        },
        &context,
    )
    .await
    .expect("bash command should time out cleanly");

    assert_eq!(result.status, BashRunStatus::TimedOut);
    assert_eq!(result.exit_code, Some(124));
    assert_eq!(result.session_id, None);
}

#[tokio::test]
async fn test_bash_timeout_terminates_process_group() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let marker = tempdir.path().join("child-finished");
    let command = format!(
        "trap '' TERM; (sleep 1; printf done > '{}') & wait",
        marker.display()
    );
    let context = crate::test_support::full_access_process_context(
        crate::ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist")),
    );

    let result = run_bash_command(
        &BashCommandRequest {
            cmd: command,
            workdir: None,
            shell: Some("sh".to_string()),
            login: false,
            tty: false,
            yield_time_ms: 5_000,
            timeout_ms: Some(50),
            max_output_tokens: None,
            sandbox_permissions: SandboxPermissions::UseDefault,
            justification: None,
            prefix_rule: None,
        },
        &context,
    )
    .await
    .expect("bash command should time out cleanly");

    assert_eq!(result.status, BashRunStatus::TimedOut);
    assert_eq!(result.exit_code, Some(124));
    assert_eq!(result.session_id, None);

    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert!(!marker.exists(), "timeout should terminate child processes");
}

#[tokio::test]
async fn test_bash_runtime_allows_escalated_command_when_called_directly() {
    let context = crate::test_support::full_access_process_context(
        crate::ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist")),
    );
    let result = run_bash_command(
        &BashCommandRequest {
            cmd: "echo hi".to_string(),
            workdir: None,
            shell: Some("sh".to_string()),
            login: false,
            tty: false,
            yield_time_ms: 50,
            timeout_ms: None,
            max_output_tokens: None,
            sandbox_permissions: SandboxPermissions::RequireEscalated,
            justification: Some("needs unrestricted filesystem access".to_string()),
            prefix_rule: None,
        },
        &context,
    )
    .await
    .expect("direct runtime execution should allow explicit sandbox escalation");

    assert!(result.output.contains("hi") || result.status == BashRunStatus::Exited);
}

#[tokio::test]
async fn test_bash_runtime_separates_stdout_stderr_and_classifies_command() {
    let context = crate::test_support::full_access_process_context(
        crate::ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist")),
    );
    let result = run_bash_command(
        &BashCommandRequest {
            cmd: "printf out; printf err >&2".to_string(),
            workdir: None,
            shell: Some("sh".to_string()),
            login: false,
            tty: false,
            yield_time_ms: 100,
            timeout_ms: None,
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
            timeout_ms: None,
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
