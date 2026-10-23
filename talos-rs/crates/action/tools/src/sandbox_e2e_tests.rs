use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use talos_config::{AgentKernelConfig, ToolSandboxNetworkConfig, ToolUserPermissionModeConfig};
use talos_tools_sandbox::{
    CommandRiskAssessment, CommandRiskCategory, CommandRiskVerdict, SandboxRuntimePaths,
    ToolSandbox, ToolSandboxAuditEvent, ToolSandboxAuditOutcome, ToolSandboxAuditSink,
    ToolSandboxAuditStage, ToolSandboxEnforcement, ToolSandboxProviderStatus,
};
use talos_types::{
    SandboxApprovalDecision, SandboxApprovalPresentation, SandboxFileOperation, TalosContentItem,
    TalosToolTaskStatus, UserActionResponse,
};

use crate::{
    register_core_local_tools, AbsolutePathBuf, CommandRiskAssessmentError,
    CommandRiskAssessmentInput, CommandRiskAssessor, ToolContext, ToolDispatchFailure,
    ToolDispatchOutcome, ToolPending, ToolRegistry, ToolSandboxEnvironment,
};

#[derive(Clone)]
struct FixedRiskAssessor {
    assessment: CommandRiskAssessment,
}

impl CommandRiskAssessor for FixedRiskAssessor {
    fn assess(
        &self,
        _input: CommandRiskAssessmentInput,
    ) -> Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<CommandRiskAssessment, CommandRiskAssessmentError>,
                > + Send
                + '_,
        >,
    > {
        let assessment = self.assessment.clone();
        Box::pin(async move { Ok(assessment) })
    }
}

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<ToolSandboxAuditEvent>>,
}

impl ToolSandboxAuditSink for RecordingSink {
    fn record(&self, event: &ToolSandboxAuditEvent) {
        println!("sandbox_audit={event:?}");
        self.events
            .lock()
            .expect("audit lock must succeed")
            .push(event.clone());
    }
}

fn sandbox() -> (ToolSandbox, Arc<RecordingSink>) {
    let sink = Arc::new(RecordingSink::default());
    let runtime_paths = SandboxRuntimePaths {
        macos_sandbox_executable: None,
        linux_helper_executable: std::env::var_os("TALOS_LINUX_SANDBOX_HELPER").map(PathBuf::from),
    };
    (
        ToolSandbox::with_runtime_paths_and_audit_sink(runtime_paths, sink.clone()),
        sink,
    )
}

fn registry(sandbox: ToolSandbox) -> ToolRegistry {
    let registry = ToolRegistry::new_with_sandbox(sandbox);
    register_core_local_tools(&registry);
    registry
}

fn context(
    root: &Path,
    mode: ToolUserPermissionModeConfig,
    network: ToolSandboxNetworkConfig,
) -> ToolContext {
    let mut config = AgentKernelConfig {
        workspace_dir: root.to_string_lossy().into_owned(),
        ..Default::default()
    };
    config.tools.sandbox_policy.user_permission_mode = mode;
    config.tools.sandbox_policy.network = network;
    let root = root
        .canonicalize()
        .expect("sandbox root should canonicalize");
    let readable_root = AbsolutePathBuf::new(
        root.ancestors()
            .last()
            .expect("workspace must have a file-system root")
            .to_path_buf(),
    )
    .expect("file-system root should be absolute");
    let environment = ToolSandboxEnvironment::local(
        vec![AbsolutePathBuf::new(root.clone()).expect("sandbox root should be absolute")],
        Vec::new(),
    )
    .expect("sandbox environment should be valid")
    .with_readable_roots(vec![readable_root]);
    ToolContext::new_with_kernel_config("sandbox-e2e", "sandbox-e2e", Some(Arc::new(config)))
        .with_turn_id("sandbox-e2e-turn")
        .with_project_root(root)
        .with_sandbox_environment(environment)
}

async fn dispatch(
    registry: &ToolRegistry,
    context: ToolContext,
    call_id: &str,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<ToolDispatchOutcome, talos_error::TalosError> {
    let invocation = registry.build_function_invocation(call_id, tool_name, arguments)?;
    registry
        .execute_invocation_with_context(invocation, context)
        .await
        .map_err(ToolDispatchFailure::into_error)
}

#[tokio::test]
#[ignore = "run through the output_logs E2E case"]
async fn bash_output_logs() {
    let workspace = tempfile::tempdir().unwrap();
    let (sandbox, _) = sandbox();
    let tools = registry(sandbox);
    let ctx = || {
        context(
            workspace.path(),
            ToolUserPermissionModeConfig::FullAccess,
            ToolSandboxNetworkConfig::Enabled,
        )
    };
    let result = dispatch(&tools, ctx(), "capture", "Bash", json!({
        "command": "python3 -c \"import sys; sys.stdout.write('START\\n' + 'log entry\\n' * 300000 + 'FINAL ERROR\\n'); sys.exit(7)\""
    })).await.unwrap();
    assert!(result.is_error());
    let envelope = result.envelope();
    let TalosContentItem::BashOutput {
        output_log: Some(log),
        stdout,
        truncated,
        ..
    } = &envelope.content_items[0]
    else {
        panic!("persisted Bash output")
    };
    assert!(*truncated);
    assert!(stdout.contains("FINAL ERROR"));
    assert!(log.stored_bytes > 2 * 1024 * 1024);
    assert_eq!(
        log.capture_status,
        talos_types::talos::BashOutputCaptureStatus::Complete
    );
    assert!(serde_json::to_vec(&envelope.content_items).unwrap().len() < 10_000);

    let found = dispatch(
        &tools,
        ctx(),
        "search",
        "Grep",
        json!({
            "path": log.path, "pattern": "FINAL ERROR", "output_mode": "content", "limit": 1
        }),
    )
    .await
    .unwrap();
    let found = found.envelope();
    let TalosContentItem::SearchResults { matches, .. } = &found.content_items[0] else {
        panic!("search page")
    };
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].line, 300002);
    let read = dispatch(
        &tools,
        ctx(),
        "read-lines",
        "Read",
        json!({"path":log.path,"offset":matches[0].line,"limit":1}),
    )
    .await
    .unwrap();
    let read = read.envelope();
    assert!(
        matches!(&read.content_items[0], TalosContentItem::FileRead { content, .. } if content == "FINAL ERROR")
    );

    // A new registry has no in-memory command state, but the reference still works.
    let (new_sandbox, _) = self::sandbox();
    let fresh = registry(new_sandbox);
    let first = dispatch(
        &fresh,
        ctx(),
        "read-bytes",
        "Read",
        json!({"path":log.path,"byte_offset":0,"max_bytes":512}),
    )
    .await
    .unwrap();
    let first = first.envelope();
    let TalosContentItem::FileRead {
        content,
        next_byte_offset: Some(next),
        ..
    } = &first.content_items[0]
    else {
        panic!("byte cursor")
    };
    assert!(content.starts_with("START\n"));
    assert_eq!(*next, 512);
    let next_page = dispatch(
        &fresh,
        ctx(),
        "continue",
        "Read",
        json!({"path":log.path,"byte_offset":next,"max_bytes":512}),
    )
    .await
    .unwrap();
    assert!(!next_page.is_error());
    assert!(std::path::Path::new(&log.path).is_file());
    // Exercise the same bounded maintenance cursor used by the Worker, then
    // verify that retention preserves the reference and TTL makes it unavailable.
    let policy = talos_config::BashOutputLogConfig::default();
    crate::runtimes::bash::maintain_bash_output_workspace(workspace.path().into(), policy.clone())
        .await
        .unwrap();
    assert!(std::path::Path::new(&log.path).is_file());
    let metadata = std::path::Path::new(&log.path).with_file_name("metadata.json");
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&metadata).unwrap()).unwrap();
    record["log"]["expiresAt"] = json!(1);
    std::fs::write(&metadata, serde_json::to_vec(&record).unwrap()).unwrap();
    let stats =
        crate::runtimes::bash::maintain_bash_output_workspace(workspace.path().into(), policy)
            .await
            .unwrap();
    assert_eq!(stats.deleted_logs, 1);
    assert!(!std::path::Path::new(&log.path).exists());
    let missing = dispatch(
        &fresh,
        ctx(),
        "expired-reference",
        "Read",
        json!({"path": log.path, "byte_offset": 0, "max_bytes": 64}),
    )
    .await;
    assert!(missing.is_err() || missing.is_ok_and(|outcome| outcome.is_error()));
}

async fn dispatch_allow_once(
    registry: &ToolRegistry,
    context: ToolContext,
    call_id: &str,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<ToolDispatchOutcome, talos_error::TalosError> {
    let invocation = registry.build_function_invocation(call_id, tool_name, arguments)?;
    let outcome = registry
        .execute_invocation_with_context(invocation.clone(), context.clone())
        .await
        .map_err(ToolDispatchFailure::into_error)?;
    let ToolDispatchOutcome::Pending {
        pending:
            ToolPending::SandboxApproval {
                request_fingerprint,
                ..
            },
        ..
    } = outcome
    else {
        return Ok(outcome);
    };

    context.set_sandbox_approval_fingerprint(call_id, request_fingerprint);
    context.set_user_action_response(
        call_id,
        UserActionResponse::SandboxApproval(SandboxApprovalDecision::AllowOnce),
    );
    registry
        .execute_invocation_with_context(invocation, context)
        .await
        .map_err(ToolDispatchFailure::into_error)
}

async fn dispatch_file_allow_once(
    registry: &ToolRegistry,
    context: ToolContext,
    call_id: &str,
    tool_name: &str,
    arguments: serde_json::Value,
    expected_operation: SandboxFileOperation,
    expected_paths: Vec<String>,
) -> Result<ToolDispatchOutcome, talos_error::TalosError> {
    let invocation = registry.build_function_invocation(call_id, tool_name, arguments)?;
    let pending = registry
        .execute_invocation_with_context(invocation.clone(), context.clone())
        .await
        .map_err(ToolDispatchFailure::into_error)?;
    let ToolDispatchOutcome::Pending {
        pending:
            ToolPending::SandboxApproval {
                params,
                request_fingerprint,
            },
        ..
    } = pending
    else {
        panic!("workspace-external {tool_name} must request approval");
    };
    assert_eq!(
        params.presentation,
        SandboxApprovalPresentation::FileSystem {
            operation: expected_operation,
            paths: expected_paths,
        }
    );

    context.set_sandbox_approval_fingerprint(call_id, request_fingerprint);
    context.set_user_action_response(
        call_id,
        UserActionResponse::SandboxApproval(SandboxApprovalDecision::AllowOnce),
    );
    registry
        .execute_invocation_with_context(invocation, context)
        .await
        .map_err(ToolDispatchFailure::into_error)
}

fn assert_local_os_event(sink: &RecordingSink) {
    let events = sink.events.lock().expect("audit lock must succeed");
    assert!(events.iter().any(|event| {
        event.stage == ToolSandboxAuditStage::SpawnProcess
            && event.outcome == ToolSandboxAuditOutcome::Succeeded
            && matches!(
                event.enforcement,
                Some(ToolSandboxEnforcement::LocalOs { .. })
            )
    }));
}

fn listener_accepts_within(listener: TcpListener, timeout: Duration, respond: bool) -> bool {
    listener
        .set_nonblocking(true)
        .expect("listener must become nonblocking");
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if respond {
                    stream
                        .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
                        .expect("listener response must be written");
                }
                return true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("listener accept failed: {error}"),
        }
    }
    false
}

fn outside_process_root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("talos-sandbox-outside-")
        .tempdir_in("/tmp")
        .expect("outside process root must be created")
}

#[tokio::test]
#[ignore = "run through the sandbox E2E group"]
async fn provider_preflight() {
    let root = tempfile::tempdir().expect("workspace must be created");
    let (sandbox, sink) = sandbox();
    let diagnostics = sandbox.diagnostics();
    println!("sandbox_diagnostics={diagnostics:?}");
    assert_eq!(diagnostics.providers.len(), 1);
    assert_eq!(
        diagnostics.providers[0].status,
        ToolSandboxProviderStatus::Available
    );
    let registry = registry(sandbox);
    let result = dispatch_allow_once(
        &registry,
        context(
            root.path(),
            ToolUserPermissionModeConfig::AskForApproval,
            ToolSandboxNetworkConfig::Restricted,
        ),
        "provider-preflight",
        "Bash",
        json!({"command": "printf sandbox-ready", "timeout": 3000}),
    )
    .await
    .expect("sandboxed Bash must execute");
    println!("tool_result={result:?}");
    assert!(!result.is_error());
    let structured = result
        .payload()
        .and_then(crate::ToolOutputPayload::structured_content)
        .expect("sandboxed Bash must return structured content");
    assert_eq!(structured["stdout"], "sandbox-ready");
    assert_eq!(structured["stderr"], "");
    assert_local_os_event(&sink);
}

#[tokio::test]
#[ignore = "run through the sandbox E2E group"]
async fn process_runtime() {
    let root = tempfile::tempdir().expect("workspace must be created");
    let (sandbox, sink) = sandbox();
    let registry = registry(sandbox);
    let result = dispatch_allow_once(
        &registry,
        context(
            root.path(),
            ToolUserPermissionModeConfig::AskForApproval,
            ToolSandboxNetworkConfig::Restricted,
        ),
        "process-runtime",
        "Bash",
        json!({
            "command": "printf runtime > \"$TMPDIR/talos-runtime-marker\" && cargo --version && if command -v node >/dev/null 2>&1; then node --version; fi && if command -v python3 >/dev/null 2>&1; then python3 --version; fi",
            "timeout": 5000
        }),
    )
    .await
    .expect("sandboxed development tools must return a tool result");
    assert!(
        !result.is_error(),
        "development tool result must be successful: {result:?}"
    );
    let structured = result
        .payload()
        .and_then(crate::ToolOutputPayload::structured_content)
        .expect("development tools must return structured Bash output");
    assert_eq!(structured["exit_code"], 0, "{structured}");
    assert!(
        structured["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("cargo"),
        "{structured}"
    );
    let stderr = structured["stderr"].as_str().unwrap_or_default();
    assert!(!stderr.contains("Operation not permitted"), "{stderr}");
    assert!(!stderr.contains("couldn't create cache file"), "{stderr}");
    assert_local_os_event(&sink);
}

#[tokio::test]
#[ignore = "run through the sandbox E2E group"]
async fn bash_terminal_status() {
    let root = tempfile::tempdir().expect("workspace must be created");
    let (sandbox, sink) = sandbox();
    let registry = registry(sandbox);
    let failed = dispatch_allow_once(
        &registry,
        context(
            root.path(),
            ToolUserPermissionModeConfig::AskForApproval,
            ToolSandboxNetworkConfig::Restricted,
        ),
        "bash-failed",
        "Bash",
        json!({"command": "printf failed-output; exit 69", "timeout": 3000}),
    )
    .await
    .expect("failed Bash must return a typed tool result");
    assert!(failed.is_error());
    assert!(matches!(
        failed.payload(),
        Some(crate::ToolOutputPayload::Function { content_items, .. })
            if matches!(
                content_items.as_slice(),
                [TalosContentItem::BashOutput {
                    status: TalosToolTaskStatus::Failed,
                    exit_code: Some(69),
                    stdout,
                    ..
                }] if stdout == "failed-output"
            )
    ));

    let timed_out = dispatch_allow_once(
        &registry,
        context(
            root.path(),
            ToolUserPermissionModeConfig::AskForApproval,
            ToolSandboxNetworkConfig::Restricted,
        ),
        "bash-timeout",
        "Bash",
        json!({"command": "sleep 1", "timeout": 50}),
    )
    .await
    .expect("timed out Bash must return a typed tool result");
    assert!(timed_out.is_error());
    assert!(matches!(
        timed_out.payload(),
        Some(crate::ToolOutputPayload::Function { content_items, .. })
            if matches!(
                content_items.as_slice(),
                [TalosContentItem::BashOutput {
                    status: TalosToolTaskStatus::TimedOut,
                    exit_code: Some(124),
                    ..
                }]
            )
    ));
    assert_local_os_event(&sink);
}

#[tokio::test]
#[ignore = "run through the sandbox E2E group"]
async fn process_filesystem() {
    let workspace = tempfile::tempdir().expect("workspace must be created");
    let outside = outside_process_root();
    let home_marker = std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("HOME must be available")
        .join(format!(
            ".talos-sandbox-write-probe-{}",
            uuid::Uuid::new_v4()
        ));
    let (sandbox, sink) = sandbox();
    let registry = registry(sandbox);
    let command = format!(
        "printf allowed > inside; (printf denied > '{}/sentinel') 2>/dev/null && exit 97; (printf denied > '{}') 2>/dev/null && exit 98; exit 69",
        outside.path().display(),
        home_marker.display()
    );
    let result = dispatch_allow_once(
        &registry,
        context(
            workspace.path(),
            ToolUserPermissionModeConfig::AskForApproval,
            ToolSandboxNetworkConfig::Restricted,
        ),
        "process-filesystem",
        "Bash",
        json!({"command": command, "timeout": 3000}),
    )
    .await
    .expect("sandboxed Bash must return a result");
    println!("tool_result={result:?}");
    assert!(result.is_error(), "denied write must fail the Bash result");
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("inside"))
            .expect("inside write must succeed"),
        "allowed"
    );
    assert!(!outside.path().join("sentinel").exists());
    let home_write_succeeded = home_marker.exists();
    if home_write_succeeded {
        std::fs::remove_file(&home_marker).expect("HOME probe must be removed");
    }
    assert!(!home_write_succeeded, "HOME must remain read-only");
    assert_local_os_event(&sink);
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "run through the sandbox E2E group"]
async fn escape_attempts() {
    let workspace = tempfile::tempdir().expect("workspace must be created");
    let outside = outside_process_root();
    std::os::unix::fs::symlink(outside.path(), workspace.path().join("escape"))
        .expect("escape symlink must be created");
    let (sandbox, sink) = sandbox();
    let registry = registry(sandbox);
    let result = dispatch_allow_once(
        &registry,
        context(
            workspace.path(),
            ToolUserPermissionModeConfig::AskForApproval,
            ToolSandboxNetworkConfig::Restricted,
        ),
        "escape-attempts",
        "Bash",
        json!({
            "command": "printf allowed > control; printf denied > escape/sentinel && exit 97; exit 69",
            "timeout": 3000
        }),
    )
    .await
    .expect("sandboxed Bash must return a result");
    println!("tool_result={result:?}");
    assert!(
        result.is_error(),
        "symlink escape must fail the Bash result"
    );
    assert!(workspace.path().join("control").exists());
    assert!(!outside.path().join("sentinel").exists());
    assert_local_os_event(&sink);
}

#[tokio::test]
#[ignore = "run through the sandbox E2E group"]
async fn network_policy() {
    let workspace = tempfile::tempdir().expect("workspace must be created");
    let enabled_listener = TcpListener::bind("127.0.0.1:0").expect("listener must bind");
    let enabled_url = format!(
        "http://{}/",
        enabled_listener
            .local_addr()
            .expect("listener address must resolve")
    );
    let (enabled_sandbox, enabled_sink) = sandbox();
    let enabled_registry = registry(enabled_sandbox);
    let enabled_command = format!("curl --max-time 2 --output /dev/null '{enabled_url}'");
    let enabled_server = std::thread::spawn(move || {
        listener_accepts_within(enabled_listener, Duration::from_secs(3), true)
    });
    let enabled = dispatch_allow_once(
        &enabled_registry,
        context(
            workspace.path(),
            ToolUserPermissionModeConfig::ApproveForMe,
            ToolSandboxNetworkConfig::Enabled,
        ),
        "network-enabled",
        "Bash",
        json!({"command": enabled_command, "timeout": 3000}),
    )
    .await
    .expect("enabled command must return");
    assert!(!enabled.is_error());
    assert!(enabled_server.join().expect("listener thread must join"));
    assert_local_os_event(&enabled_sink);

    let restricted_workspace = tempfile::tempdir().expect("workspace must be created");
    let restricted_listener = TcpListener::bind("127.0.0.1:0").expect("listener must bind");
    let restricted_url = format!(
        "http://{}/",
        restricted_listener
            .local_addr()
            .expect("listener address must resolve")
    );
    let (restricted_sandbox, restricted_sink) = sandbox();
    let restricted_registry = registry(restricted_sandbox);
    let restricted_command = format!("curl --max-time 1 --output /dev/null '{restricted_url}'");
    let restricted = dispatch_allow_once(
        &restricted_registry,
        context(
            restricted_workspace.path(),
            ToolUserPermissionModeConfig::AskForApproval,
            ToolSandboxNetworkConfig::Restricted,
        ),
        "network-restricted",
        "Bash",
        json!({"command": restricted_command, "timeout": 2500}),
    )
    .await
    .expect("restricted command must return");
    assert!(restricted.is_error());
    assert!(!listener_accepts_within(
        restricted_listener,
        Duration::from_millis(300),
        false,
    ));
    assert_local_os_event(&restricted_sink);
}

#[tokio::test]
#[ignore = "run through the sandbox E2E group"]
async fn file_tools() {
    let workspace = tempfile::tempdir().expect("workspace must be created");
    let outside = tempfile::tempdir().expect("outside root must be created");
    std::fs::write(workspace.path().join("readable.txt"), "workspace")
        .expect("workspace fixture must be written");
    std::fs::write(outside.path().join("secret.txt"), "outside")
        .expect("outside fixture must be written");
    std::fs::write(outside.path().join("editable.txt"), "before")
        .expect("outside edit fixture must be written");
    let (sandbox, sink) = sandbox();
    let registry = registry(sandbox);
    let local_context = context(
        workspace.path(),
        ToolUserPermissionModeConfig::AskForApproval,
        ToolSandboxNetworkConfig::Restricted,
    );

    let readable = dispatch(
        &registry,
        local_context.clone(),
        "file-tools-read-control",
        "Read",
        json!({"path": "readable.txt"}),
    )
    .await
    .expect("workspace read must succeed");
    assert!(!readable.is_error());
    let searched = dispatch(
        &registry,
        local_context.clone(),
        "file-tools-search-control",
        "Grep",
        json!({
            "path": workspace.path().join("readable.txt").display().to_string(),
            "pattern": "workspace",
            "output_mode": "content"
        }),
    )
    .await
    .expect("single-file workspace search must succeed");
    assert!(!searched.is_error());
    let globbed = dispatch(
        &registry,
        local_context.clone(),
        "file-tools-glob-control",
        "Glob",
        json!({"path": workspace.path(), "pattern": "*.txt"}),
    )
    .await
    .expect("workspace glob must succeed");
    assert!(!globbed.is_error());

    dispatch(
        &registry,
        local_context.clone(),
        "file-tools-control",
        "Write",
        json!({"path": "inside.txt", "content": "allowed"}),
    )
    .await
    .expect("workspace write must succeed");
    dispatch(
        &registry,
        local_context.clone(),
        "file-tools-read-edit-control",
        "Read",
        json!({"path": "inside.txt"}),
    )
    .await
    .expect("workspace edit prerequisite read must succeed");
    dispatch(
        &registry,
        local_context.clone(),
        "file-tools-edit-control",
        "Edit",
        json!({"path": "inside.txt", "old_string": "allowed", "new_string": "edited"}),
    )
    .await
    .expect("workspace edit must succeed");

    let outside_read_context = context(
        workspace.path(),
        ToolUserPermissionModeConfig::AskForApproval,
        ToolSandboxNetworkConfig::Restricted,
    );
    let outside_secret = outside
        .path()
        .join("secret.txt")
        .canonicalize()
        .expect("outside read path must canonicalize");
    let approved_read = dispatch_file_allow_once(
        &registry,
        outside_read_context,
        "file-tools-read-approved",
        "Read",
        json!({"path": outside_secret}),
        SandboxFileOperation::Read,
        vec![outside_secret.display().to_string()],
    )
    .await
    .expect("approved outside read must execute");
    assert!(!approved_read.is_error());

    let outside_glob_root = outside
        .path()
        .canonicalize()
        .expect("outside glob root must canonicalize");
    let approved_glob = dispatch_file_allow_once(
        &registry,
        context(
            workspace.path(),
            ToolUserPermissionModeConfig::AskForApproval,
            ToolSandboxNetworkConfig::Restricted,
        ),
        "file-tools-glob-approved",
        "Glob",
        json!({"path": outside_glob_root, "pattern": "*.txt"}),
        SandboxFileOperation::Read,
        vec![outside_glob_root.display().to_string()],
    )
    .await
    .expect("approved outside glob must execute");
    assert!(!approved_glob.is_error());

    let outside_write = outside.path().join("written.txt");
    let approved_write = dispatch_file_allow_once(
        &registry,
        context(
            workspace.path(),
            ToolUserPermissionModeConfig::AskForApproval,
            ToolSandboxNetworkConfig::Restricted,
        ),
        "file-tools-write-approved",
        "Write",
        json!({"path": outside_write, "content": "outside-write"}),
        SandboxFileOperation::Write,
        vec![outside_glob_root.display().to_string()],
    )
    .await
    .expect("approved outside write must execute");
    assert!(!approved_write.is_error());

    let outside_edit = outside
        .path()
        .join("editable.txt")
        .canonicalize()
        .expect("outside edit path must canonicalize");
    let outside_edit_context = context(
        workspace.path(),
        ToolUserPermissionModeConfig::AskForApproval,
        ToolSandboxNetworkConfig::Restricted,
    );
    dispatch_file_allow_once(
        &registry,
        outside_edit_context.clone(),
        "file-tools-edit-read-approved",
        "Read",
        json!({"path": outside_edit}),
        SandboxFileOperation::Read,
        vec![outside_edit.display().to_string()],
    )
    .await
    .expect("approved outside edit prerequisite read must execute");
    let approved_edit = dispatch_file_allow_once(
        &registry,
        outside_edit_context,
        "file-tools-edit-approved",
        "Edit",
        json!({"path": outside_edit, "old_string": "before", "new_string": "after"}),
        SandboxFileOperation::Write,
        vec![outside_edit.display().to_string()],
    )
    .await
    .expect("approved outside edit must execute");
    assert!(!approved_edit.is_error());

    assert_eq!(
        std::fs::read_to_string(workspace.path().join("inside.txt"))
            .expect("inside file must exist"),
        "edited"
    );
    assert_eq!(
        std::fs::read_to_string(outside_write).expect("outside write must exist"),
        "outside-write"
    );
    assert_eq!(
        std::fs::read_to_string(outside_edit).expect("outside edit must exist"),
        "after"
    );
    let events = sink.events.lock().expect("audit lock must succeed");
    assert!(events.iter().any(|event| {
        event.stage == ToolSandboxAuditStage::Evaluate
            && event.outcome == ToolSandboxAuditOutcome::RequiresApproval
    }));
    assert!(events.iter().any(|event| {
        event.stage == ToolSandboxAuditStage::FileSystem
            && event.outcome == ToolSandboxAuditOutcome::Succeeded
            && event.enforcement == Some(ToolSandboxEnforcement::InProcessAuthorizer)
    }));
}

#[tokio::test]
#[ignore = "run through the sandbox E2E group"]
async fn full_access_control() {
    let workspace = tempfile::tempdir().expect("workspace must be created");
    let outside = tempfile::tempdir().expect("outside root must be created");
    let (sandbox, sink) = sandbox();
    let registry = registry(sandbox);
    dispatch(
        &registry,
        context(
            workspace.path(),
            ToolUserPermissionModeConfig::FullAccess,
            ToolSandboxNetworkConfig::Enabled,
        ),
        "full-access-control",
        "Write",
        json!({
            "path": outside.path().join("sentinel").display().to_string(),
            "content": "full-access"
        }),
    )
    .await
    .expect("full access write must succeed");
    let bash_marker = outside.path().join("bash-sentinel");
    let bash_result = dispatch(
        &registry,
        context(
            workspace.path(),
            ToolUserPermissionModeConfig::FullAccess,
            ToolSandboxNetworkConfig::Enabled,
        ),
        "full-access-bash-control",
        "Bash",
        json!({
            "command": format!("printf full-access-bash > '{}'", bash_marker.display()),
            "timeout": 3000
        }),
    )
    .await
    .expect("full access Bash must succeed");
    assert!(!bash_result.is_error());
    assert_eq!(
        std::fs::read_to_string(outside.path().join("sentinel")).expect("outside file must exist"),
        "full-access"
    );
    assert_eq!(
        std::fs::read_to_string(&bash_marker).expect("Bash marker must exist"),
        "full-access-bash"
    );
    let events = sink.events.lock().expect("audit lock must succeed");
    assert!(events.iter().any(|event| {
        event.stage == ToolSandboxAuditStage::FileSystem
            && event.outcome == ToolSandboxAuditOutcome::Succeeded
            && event.enforcement == Some(ToolSandboxEnforcement::None)
    }));
    assert!(events.iter().any(|event| {
        event.stage == ToolSandboxAuditStage::SpawnProcess
            && event.outcome == ToolSandboxAuditOutcome::Succeeded
            && event.enforcement == Some(ToolSandboxEnforcement::None)
    }));
}

#[tokio::test]
#[ignore = "run through the sandbox E2E group"]
async fn approve_for_me_risk() {
    let workspace = tempfile::tempdir().expect("workspace must be created");
    let (sandbox, sink) = sandbox();
    let registry = registry(sandbox);
    let safe_context = context(
        workspace.path(),
        ToolUserPermissionModeConfig::ApproveForMe,
        ToolSandboxNetworkConfig::Enabled,
    )
    .with_command_risk_assessor(Arc::new(FixedRiskAssessor {
        assessment: CommandRiskAssessment {
            verdict: CommandRiskVerdict::Safe,
            categories: Vec::new(),
            summary: "No command risk identified".to_string(),
        },
    }));
    let safe = dispatch(
        &registry,
        safe_context,
        "approve-for-me-safe",
        "Bash",
        json!({"command": "printf safe > safe-marker", "timeout": 3000}),
    )
    .await
    .expect("structured-safe command must execute");
    assert!(!safe.is_error());
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("safe-marker"))
            .expect("safe marker must exist"),
        "safe"
    );

    let risky_marker = workspace.path().join("risky-marker");
    let risky_context = context(
        workspace.path(),
        ToolUserPermissionModeConfig::ApproveForMe,
        ToolSandboxNetworkConfig::Enabled,
    )
    .with_command_risk_assessor(Arc::new(FixedRiskAssessor {
        assessment: CommandRiskAssessment {
            verdict: CommandRiskVerdict::Risky,
            categories: vec![CommandRiskCategory::ProcessOrServiceControl],
            summary: "Command effects require user review".to_string(),
        },
    }));
    let invocation = registry
        .build_function_invocation(
            "approve-for-me-risky",
            "Bash",
            json!({
                "command": format!("printf risky > '{}'", risky_marker.display()),
                "timeout": 3000
            }),
        )
        .expect("risky invocation must build");
    let pending = registry
        .execute_invocation_with_context(invocation.clone(), risky_context.clone())
        .await
        .expect("structured-risky command must request approval");
    let ToolDispatchOutcome::Pending {
        pending:
            ToolPending::SandboxApproval {
                params,
                request_fingerprint,
            },
        ..
    } = pending
    else {
        panic!("structured-risky command must be pending");
    };
    let talos_types::SandboxApprovalPresentation::Command {
        command,
        cwd,
        risk_summary,
    } = params.presentation
    else {
        panic!("risky Bash approval must use command presentation");
    };
    assert_eq!(
        command,
        format!("printf risky > '{}'", risky_marker.display())
    );
    assert_eq!(
        PathBuf::from(cwd),
        workspace
            .path()
            .canonicalize()
            .expect("workspace path must canonicalize")
    );
    assert_eq!(
        risk_summary.as_deref(),
        Some("Command effects require user review")
    );
    assert!(!risky_marker.exists());

    risky_context.set_sandbox_approval_fingerprint("approve-for-me-risky", request_fingerprint);
    risky_context.set_user_action_response(
        "approve-for-me-risky",
        UserActionResponse::SandboxApproval(SandboxApprovalDecision::AllowOnce),
    );
    let resumed = registry
        .execute_invocation_with_context(invocation, risky_context)
        .await
        .expect("approved risky command must execute");
    assert!(!resumed.is_error());
    assert_eq!(
        std::fs::read_to_string(risky_marker).expect("risky marker must exist after approval"),
        "risky"
    );
    let events = sink.events.lock().expect("audit lock must succeed");
    assert!(events.iter().any(|event| {
        event.stage == ToolSandboxAuditStage::Evaluate
            && event.outcome == ToolSandboxAuditOutcome::NeedsRiskAssessment
    }));
    assert!(events.iter().any(|event| {
        event.stage == ToolSandboxAuditStage::Evaluate
            && event.outcome == ToolSandboxAuditOutcome::RequiresApproval
    }));
}

#[tokio::test]
#[ignore = "run through the sandbox E2E group"]
async fn no_silent_fallback() {
    let workspace = tempfile::tempdir().expect("workspace root must be created");
    let outside = tempfile::tempdir().expect("outside root must be created");
    let missing_provider_executable = workspace.path().join("missing-sandbox-provider");
    let sink = Arc::new(RecordingSink::default());
    let sandbox = ToolSandbox::with_runtime_paths_and_audit_sink(
        SandboxRuntimePaths {
            macos_sandbox_executable: Some(missing_provider_executable.clone()),
            linux_helper_executable: Some(missing_provider_executable),
        },
        sink.clone(),
    );
    let diagnostics = sandbox.diagnostics();
    println!("sandbox_diagnostics={diagnostics:?}");
    assert!(diagnostics
        .providers
        .iter()
        .any(|provider| { matches!(provider.status, ToolSandboxProviderStatus::Failed { .. }) }));
    let registry = registry(sandbox);
    let command = format!("printf fallback > '{}/sentinel'", outside.path().display());
    let result = dispatch_allow_once(
        &registry,
        context(
            workspace.path(),
            ToolUserPermissionModeConfig::AskForApproval,
            ToolSandboxNetworkConfig::Restricted,
        ),
        "no-silent-fallback",
        "Bash",
        json!({"command": command, "timeout": 3000}),
    )
    .await;
    assert!(result.is_err());
    assert!(!outside.path().join("sentinel").exists());
    let events = sink.events.lock().expect("audit lock must succeed");
    assert!(events.iter().any(|event| {
        event.stage == ToolSandboxAuditStage::SpawnProcess
            && event.outcome == ToolSandboxAuditOutcome::Failed
            && event.error_category
                == Some(talos_tools_sandbox::TalosSandboxErrorCategory::SetupFailed)
    }));
    assert!(!events.iter().any(|event| {
        event.stage == ToolSandboxAuditStage::SpawnProcess
            && event.outcome == ToolSandboxAuditOutcome::Succeeded
            && event.enforcement == Some(ToolSandboxEnforcement::None)
    }));
}
