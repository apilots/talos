use super::*;

fn completed_session(capture: OutputCapture) -> Arc<BashSession> {
    Arc::new(BashSession {
        session_id: 0,
        termination_target: None,
        state: Mutex::new(BashSessionState {
            exit_code: Some(0),
            termination: None,
        }),
        output: Mutex::new(capture),
        notify: Notify::new(),
        started_at: Instant::now(),
        command_kind: BashCommandKind::ReadOnly,
        risk: BashCommandRisk::Low,
        progress_emitter: None,
        cancellation: None,
        reader_stop: tokio_util::sync::CancellationToken::new(),
        readers_remaining: AtomicUsize::new(1),
        abandoned: AtomicBool::new(false),
        tasks: Mutex::new(None),
    })
}

#[tokio::test]
#[cfg(unix)]
async fn forced_invocation_abort_releases_the_session_and_finalizes_received_output() {
    use talos_fs::FileSystem;
    let workspace = tempfile::tempdir().unwrap();
    let config = talos_config::AgentKernelConfig {
        workspace_dir: workspace.path().to_string_lossy().into_owned(),
        ..Default::default()
    };
    let mut context = crate::test_support::full_access_process_context(
        crate::ToolContext::new_with_kernel_config("forced-abort", "user", Some(Arc::new(config)))
            .with_turn_id("turn")
            .with_project_root(workspace.path()),
    );
    context.bind_current_call_id("abort");
    let request = BashCommandRequest {
        cmd:
            "python3 -c \"print('x' * 20000)\"; touch ready; (sleep 1; touch child-finished) & wait"
                .into(),
        workdir: None,
        shell: Some("sh".into()),
        login: false,
        tty: false,
        yield_time_ms: 5000,
        timeout_ms: Some(10000),
        max_output_bytes: None,
        sandbox_permissions: SandboxPermissions::UseDefault,
        justification: None,
        prefix_rule: None,
    };
    let task = tokio::spawn(async move {
        run_bash_command_with_policy(
            &request,
            &context,
            crate::spec::ToolResultPolicy {
                max_inline_bytes: 1024,
                preview_lines: 60,
                persist_large_output: true,
            },
        )
        .await
    });
    let path = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let mut directories = vec![workspace.path().join("tool-output")];
            for _ in 0..3 {
                directories = directories
                    .iter()
                    .flat_map(|path| std::fs::read_dir(path).into_iter().flatten())
                    .filter_map(Result::ok)
                    .filter(|entry| entry.path().is_dir())
                    .map(|entry| entry.path())
                    .collect();
            }
            if workspace.path().join("ready").exists() {
                if let Some(directory) = directories
                    .into_iter()
                    .find(|path| path.join("metadata.json").is_file())
                {
                    break directory;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(_lock) = talos_fs::std_fs()
                .try_file_lock(&path.join("capture.lock"))
                .unwrap()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("forced abort must release its log reservation");
    assert!(BashSessionManager::global().sessions.is_empty());
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.join("metadata.json")).unwrap()).unwrap();
    assert_eq!(record["log"]["captureStatus"], "interrupted");
    assert!(!std::fs::read(path.join("output.log")).unwrap().is_empty());
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(!workspace.path().join("child-finished").exists());
}

#[tokio::test]
async fn pipe_read_failure_is_retained_as_a_capture_error() {
    struct FailedPipe;
    impl AsyncRead for FailedPipe {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            _: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Err(std::io::Error::other("fixture pipe failure")))
        }
    }
    let capture = OutputCapture::new(
        &crate::ToolContext::default(),
        crate::spec::ToolResultPolicy {
            max_inline_bytes: 1024,
            preview_lines: 60,
            persist_large_output: false,
        },
    )
    .await
    .unwrap();
    let session = completed_session(capture);
    spawn_reader(FailedPipe, session.clone(), OutputStreamKind::Stderr)
        .await
        .unwrap();
    let capture = session.output.lock().await;
    assert!(capture.interrupted);
    let error = capture
        .error
        .as_ref()
        .expect("pipe failure must be visible");
    assert!(error.is_code(talos_error::ErrorCode::InternalError));
    assert!(!error
        .to_api_error()
        .message
        .contains("fixture pipe failure"));
}

#[tokio::test]
async fn reader_drain_does_not_abort_an_inflight_initial_spill() {
    use talos_fs::FileSystem;
    use tokio::io::AsyncWriteExt;
    let workspace = tempfile::tempdir().unwrap();
    let config = talos_config::AgentKernelConfig {
        workspace_dir: workspace.path().to_string_lossy().into_owned(),
        ..Default::default()
    };
    let mut context =
        crate::ToolContext::new_with_kernel_config("session", "user", Some(Arc::new(config)))
            .with_turn_id("turn");
    context.bind_current_call_id("slow-spill");
    let capture = OutputCapture::new(
        &context,
        crate::spec::ToolResultPolicy {
            max_inline_bytes: 1024,
            preview_lines: 60,
            persist_large_output: true,
        },
    )
    .await
    .unwrap();
    output_log::LogWriter::maintain_workspace(
        workspace.path().into(),
        talos_config::BashOutputLogConfig::default(),
    )
    .await
    .unwrap();
    let maintenance_lock = talos_fs::std_fs()
        .try_file_lock(&workspace.path().join("tool-output/workspace.lock"))
        .unwrap()
        .unwrap();
    let session = completed_session(capture);
    let (mut pipe, reader) = tokio::io::duplex(4096);
    let bytes = vec![b'x'; 2048];
    pipe.write_all(&bytes).await.unwrap();
    let reader = spawn_reader(reader, session.clone(), OutputStreamKind::Stdout);
    tokio::time::timeout(Duration::from_secs(5), async {
        while session.output.try_lock().is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    session.tasks.lock().await.replace(BashSessionTasks {
        readers: vec![reader],
        waiter: tokio::spawn(async {}),
        timeout: None,
    });
    tokio::join!(join_session_tasks(&session), async {
        tokio::time::sleep(Duration::from_millis(TASK_JOIN_GRACE_MS * 3)).await;
        drop(maintenance_lock);
    });
    let mut capture = session.output.lock().await;
    capture.finish().await;
    let log = capture
        .log
        .as_ref()
        .expect("inflight spill must remain owned through drain");
    assert_eq!(std::fs::read(&log.path).unwrap(), bytes);
    assert_eq!(
        log.capture_status,
        talos_types::BashOutputCaptureStatus::Interrupted
    );
}

#[tokio::test]
async fn mixed_stream_progress_has_one_shared_decoded_text_budget() {
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = events.clone();
    let context = crate::test_support::full_access_process_context(
        crate::ToolContext::default()
            .with_project_root(std::env::current_dir().unwrap())
            .with_progress_emitter(crate::ToolProgressEmitter::new(move |event| {
                sink.lock().unwrap().push(event);
            })),
    );
    let request = BashCommandRequest {
        cmd: "python3 -c \"import os; os.write(1, bytes([255]) * 20000); os.write(2, b'err' * 10000)\"".into(),
        workdir: None,
        shell: Some("sh".into()),
        login: false,
        tty: false,
        yield_time_ms: 5000,
        timeout_ms: Some(5000),
        max_output_bytes: None,
        sandbox_permissions: SandboxPermissions::UseDefault,
        justification: None,
        prefix_rule: None,
    };
    let result = run_bash_command_with_policy(
        &request,
        &context,
        crate::spec::ToolResultPolicy {
            max_inline_bytes: 256,
            preview_lines: 60,
            persist_large_output: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, Some(0));
    let events = events.lock().unwrap();
    assert!(events.iter().map(|event| event.text.len()).sum::<usize>() <= 256);
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == crate::ToolProgressKind::Status)
            .count(),
        1
    );
    assert!(events
        .iter()
        .any(|event| event.kind == crate::ToolProgressKind::Stdout));
}

#[tokio::test]
async fn unavailable_log_reservation_keeps_the_executed_command_status() {
    use crate::Tool;
    let workspace = tempfile::tempdir().unwrap();
    let mut config = talos_config::AgentKernelConfig {
        workspace_dir: workspace.path().to_string_lossy().into_owned(),
        ..Default::default()
    };
    config.tools.bash.output_log = talos_config::BashOutputLogConfig {
        max_file_bytes: 32,
        max_workspace_bytes: 4128,
        retention_days: 7,
        cleanup_interval_secs: 3600,
    };
    let _active = output_log::LogWriter::open(
        workspace.path().into(),
        "session".into(),
        "turn".into(),
        "active".into(),
        config.tools.bash.output_log.clone(),
    )
    .await
    .unwrap();
    let mut context = crate::test_support::full_access_process_context(
        crate::ToolContext::new_with_kernel_config("session", "user", Some(Arc::new(config)))
            .with_turn_id("turn")
            .with_project_root(workspace.path()),
    );
    context.bind_current_call_id("no-reservation");
    let request = BashCommandRequest {
        cmd: "printf 'an executed command with output'".into(),
        workdir: None,
        shell: Some("sh".into()),
        login: false,
        tty: false,
        yield_time_ms: 5000,
        timeout_ms: Some(5000),
        max_output_bytes: None,
        sandbox_permissions: SandboxPermissions::UseDefault,
        justification: None,
        prefix_rule: None,
    };
    let result = run_bash_command_with_policy(
        &request,
        &context,
        crate::spec::ToolResultPolicy {
            max_inline_bytes: 16,
            preview_lines: 60,
            persist_large_output: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(result.status, BashRunStatus::Exited);
    assert_eq!(result.exit_code, Some(0));
    assert!(result.output_log.is_none());
    assert_eq!(
        result.capture_error.unwrap().code,
        talos_error::ErrorCode::UsageLimitExceeded
    );
    let payload = crate::BashTool
        .execute_payload(
            serde_json::json!({
                "command": "python3 -c \"print('x' * 20000)\""
            }),
            context,
        )
        .await
        .unwrap();
    assert!(payload.is_error());
    let envelope = payload.envelope();
    assert!(!envelope.error.as_ref().unwrap().retryable);
    assert!(matches!(
        &envelope.content_items[0],
        talos_types::TalosContentItem::BashOutput {
            exit_code: Some(0),
            status: talos_types::TalosToolTaskStatus::Completed,
            ..
        }
    ));
}

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
            max_output_bytes: None,
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
            max_output_bytes: Some(100),
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
            max_output_bytes: None,
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
            max_output_bytes: None,
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
async fn run_cancellation_terminates_the_bash_process_group() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let marker = tempdir.path().join("child-finished");
    let command = format!(
        "trap '' TERM; (sleep 1; printf done > '{}') & wait",
        marker.display()
    );
    let requested = tokio_util::sync::CancellationToken::new();
    let observed = requested.clone();
    let context = crate::test_support::full_access_process_context(
        crate::ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist"))
            .with_cancellation(crate::InvocationCancellation::new(async move {
                observed.cancelled().await
            })),
    );
    let cancel_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        requested.cancel();
    });
    let result = run_bash_command(
        &BashCommandRequest {
            cmd: command,
            workdir: None,
            shell: Some("sh".to_string()),
            login: false,
            tty: false,
            yield_time_ms: 5_000,
            timeout_ms: Some(5_000),
            max_output_bytes: None,
            sandbox_permissions: SandboxPermissions::UseDefault,
            justification: None,
            prefix_rule: None,
        },
        &context,
    )
    .await
    .expect("Bash cancellation should settle cleanly");
    cancel_task.await.expect("cancellation task should finish");

    assert_eq!(result.status, BashRunStatus::Cancelled);
    assert_eq!(result.exit_code, Some(130));
    assert_eq!(result.session_id, None);
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert!(
        !marker.exists(),
        "cancellation should terminate child processes"
    );
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
            max_output_bytes: None,
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
            max_output_bytes: None,
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
            max_output_bytes: None,
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
