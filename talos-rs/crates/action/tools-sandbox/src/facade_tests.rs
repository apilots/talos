use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use talos_sandbox_spi::{
    default_request_support, NetworkIsolationSpec, PreparedSandboxCommand, SandboxExecutionRequest,
    SandboxProvider, SandboxProviderCapabilities, SandboxProviderError, SandboxProviderSnapshot,
    SandboxRequestSupport,
};
use talos_types::UserPermissionMode;

use super::*;
use crate::{
    AbsolutePathBuf, FileSystemPermissionLevel, NetworkSandboxPolicy, RequestedToolPermissions,
    SandboxStdin, TalosSandboxErrorCategory, ToolExecutionLocation, ToolGovernanceConstraints,
    ToolInvocationIdentity, ToolProcessCommand, ToolProcessTerminationTarget,
    ToolSandboxAuditEvent, ToolSandboxAuditOutcome, ToolSandboxAuditSink, ToolSandboxAuditStage,
    ToolSandboxEnvironment, ToolSandboxOperationKind,
};

#[derive(Default)]
struct RecordingAuditSink {
    events: Mutex<Vec<ToolSandboxAuditEvent>>,
}

impl ToolSandboxAuditSink for RecordingAuditSink {
    fn record(&self, event: &ToolSandboxAuditEvent) {
        self.events
            .lock()
            .expect("audit event lock must succeed")
            .push(event.clone());
    }
}

struct FakeProvider {
    probe_count: AtomicUsize,
    prepared_request: Mutex<Option<SandboxExecutionRequest>>,
    fail_probe: bool,
    fail_prepare: bool,
    unsupported: bool,
}

impl FakeProvider {
    fn available() -> Self {
        Self {
            probe_count: AtomicUsize::new(0),
            prepared_request: Mutex::new(None),
            fail_probe: false,
            fail_prepare: false,
            unsupported: false,
        }
    }

    fn failing_prepare() -> Self {
        Self {
            fail_prepare: true,
            ..Self::available()
        }
    }
}

impl SandboxProvider for FakeProvider {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn supports_platform(&self, platform: SandboxProviderPlatform) -> bool {
        platform == SandboxProviderPlatform::MacosSeatbelt
    }

    fn probe(
        &self,
        platform: SandboxProviderPlatform,
    ) -> Result<SandboxProviderCapabilities, SandboxProviderError> {
        self.probe_count.fetch_add(1, Ordering::SeqCst);
        if self.fail_probe {
            return Err(SandboxProviderError::ProbeFailed {
                reason: "probe sentinel".to_string(),
            });
        }
        Ok(SandboxProviderCapabilities::all(platform, false))
    }

    fn support(
        &self,
        request: &SandboxExecutionRequest,
        snapshot: &SandboxProviderSnapshot,
    ) -> SandboxRequestSupport {
        if self.unsupported {
            return SandboxRequestSupport::Unsupported {
                capability: SandboxIsolationCapability::FileSystemRead,
                reason: "unsupported sentinel".to_string(),
            };
        }
        default_request_support(request, snapshot)
    }

    fn prepare(
        &self,
        request: SandboxExecutionRequest,
    ) -> Result<PreparedSandboxCommand, SandboxProviderError> {
        if self.fail_prepare {
            return Err(SandboxProviderError::PrepareFailed {
                reason: "prepare sentinel".to_string(),
            });
        }
        self.prepared_request
            .lock()
            .expect("prepared request lock must succeed")
            .replace(request.clone());
        Ok(PreparedSandboxCommand::from_command(request.into_command()))
    }
}

fn sandbox(provider: Arc<dyn SandboxProvider>) -> ToolSandbox {
    ToolSandbox::from_providers(Some(SandboxProviderPlatform::MacosSeatbelt), vec![provider])
}

fn request(
    root: &tempfile::TempDir,
    mode: UserPermissionMode,
    file_system: FileSystemPermissionLevel,
    operations: BTreeSet<ToolSandboxOperationKind>,
) -> ToolSandboxRequest {
    let workspace = AbsolutePathBuf::new(root.path().to_path_buf())
        .expect("temporary workspace must be absolute");
    let denied = AbsolutePathBuf::new(root.path().join("denied"))
        .expect("temporary denied path must be absolute");
    ToolSandboxRequest {
        invocation: ToolInvocationIdentity::new("call-1", "Bash"),
        user_mode: mode,
        governance: ToolGovernanceConstraints::default(),
        requested_permissions: RequestedToolPermissions::new(
            file_system,
            NetworkSandboxPolicy::Restricted,
            None,
            operations,
        )
        .expect("request permissions must be valid"),
        approval_grant: None,
        command_risk_assessment: None,
        environment: ToolSandboxEnvironment {
            readable_roots: vec![workspace.clone()],
            workspace_roots: vec![workspace],
            denied_roots: vec![denied],
            location: ToolExecutionLocation::Local,
        },
    }
}

fn process_command(cwd: &std::path::Path) -> ToolProcessCommand {
    ToolProcessCommand {
        program: env::current_exe().expect("current executable must resolve"),
        args: vec!["--list".to_string()],
        env: BTreeMap::new(),
        cwd: cwd.to_path_buf(),
        stdin: SandboxStdin::Closed,
    }
}

#[tokio::test]
async fn evaluate_spawn_uses_provider_and_cached_probe_snapshot() {
    let root = tempfile::tempdir().expect("temporary workspace must be created");
    let provider = Arc::new(FakeProvider::available());
    let sandbox = sandbox(provider.clone());
    assert_eq!(provider.probe_count.load(Ordering::SeqCst), 1);

    let decision = sandbox
        .evaluate(request(
            &root,
            UserPermissionMode::AskForApproval,
            FileSystemPermissionLevel::ReadOnly,
            BTreeSet::from([ToolSandboxOperationKind::SpawnProcess]),
        ))
        .expect("request must evaluate");
    let ToolSandboxDecision::Allowed(permit) = decision else {
        panic!("request must be allowed");
    };
    let mut command = process_command(root.path());
    let host_home = root.path().join("host-home");
    std::fs::create_dir(&host_home).expect("host HOME must be created");
    command
        .env
        .insert("PATH".to_string(), "/usr/bin".to_string());
    command
        .env
        .insert("HOME".to_string(), host_home.to_string_lossy().into_owned());
    command
        .env
        .insert("SECRET_SENTINEL".to_string(), "hidden".to_string());
    let mut process = permit
        .spawn(command)
        .await
        .expect("provider command must spawn");
    #[cfg(unix)]
    assert_eq!(
        process.termination_target(),
        process.id().map(ToolProcessTerminationTarget::ProcessGroup),
        "spawned tools must own a process group isolated from the test process"
    );
    assert!(process.wait().await.expect("process must wait").success());
    assert_eq!(
        process.enforcement(),
        &ToolSandboxEnforcement::LocalOs {
            platform: SandboxProviderPlatform::MacosSeatbelt,
            provider_name: "fake".to_string(),
        }
    );
    assert_eq!(provider.probe_count.load(Ordering::SeqCst), 1);
    let prepared = provider
        .prepared_request
        .lock()
        .expect("prepared request lock must succeed")
        .clone();
    let prepared = prepared
        .as_ref()
        .expect("provider request must be captured");
    let private_tmp = PathBuf::from(
        prepared
            .command()
            .env()
            .get("TMPDIR")
            .expect("enforcing command must receive a private TMPDIR"),
    );
    let prepared_home = PathBuf::from(
        prepared
            .command()
            .env()
            .get("HOME")
            .expect("enforcing command must retain HOME"),
    );
    assert_eq!(
        prepared.command().env().get("PATH").map(String::as_str),
        Some("/usr/bin")
    );
    assert_eq!(
        prepared
            .command()
            .env()
            .get("xcrun_nocache")
            .map(String::as_str),
        Some("1")
    );
    assert!(!prepared.command().env().contains_key("SECRET_SENTINEL"));
    assert!(prepared.runtime().private_tmp);
    assert_eq!(prepared_home, host_home);
    assert_eq!(
        prepared.file_system().access_for(
            &AbsolutePathBuf::new(private_tmp.clone()).expect("private TMPDIR must be absolute")
        ),
        talos_sandbox_spi::SandboxPathAccess::Writable
    );
    if let Some(host_tmp) = std::env::var_os("TMPDIR") {
        let host_tmp = AbsolutePathBuf::new(
            std::fs::canonicalize(host_tmp).expect("host TMPDIR should canonicalize"),
        )
        .expect("host TMPDIR should be absolute");
        assert_eq!(
            prepared.file_system().access_for(&host_tmp),
            talos_sandbox_spi::SandboxPathAccess::Denied
        );
    }
    let expected_program = AbsolutePathBuf::new(
        std::fs::canonicalize(env::current_exe().expect("current executable must resolve"))
            .expect("current executable must canonicalize"),
    )
    .expect("canonical program must be absolute");
    let expected_root = AbsolutePathBuf::new(
        std::fs::canonicalize(root.path()).expect("workspace must canonicalize"),
    )
    .expect("canonical workspace must be absolute");
    assert_eq!(prepared.command().program(), &expected_program);
    assert_eq!(prepared.command().args(), &["--list".to_string()]);
    assert_eq!(prepared.command().cwd(), &expected_root);
    assert_eq!(prepared.network(), NetworkIsolationSpec::Restricted);
    assert!(private_tmp.is_dir());
    drop(process);
    assert!(!private_tmp.exists());
}

#[tokio::test]
async fn enforcing_process_can_write_private_tmp_until_process_is_dropped() {
    let root = tempfile::tempdir().expect("temporary workspace must be created");
    let provider = Arc::new(FakeProvider::available());
    let sandbox = sandbox(provider.clone());
    let ToolSandboxDecision::Allowed(permit) = sandbox
        .evaluate(request(
            &root,
            UserPermissionMode::AskForApproval,
            FileSystemPermissionLevel::ReadOnly,
            BTreeSet::from([ToolSandboxOperationKind::SpawnProcess]),
        ))
        .expect("request must evaluate")
    else {
        panic!("request must be allowed");
    };
    let command = ToolProcessCommand {
        program: "/bin/sh".into(),
        args: vec![
            "-c".to_string(),
            "test -d \"$TMPDIR\" && printf private > \"$TMPDIR/marker\"".to_string(),
        ],
        env: BTreeMap::new(),
        cwd: root.path().to_path_buf(),
        stdin: SandboxStdin::Closed,
    };

    let mut process = permit
        .spawn(command)
        .await
        .expect("enforcing process must spawn");
    assert!(process.wait().await.expect("process must wait").success());
    let prepared = provider
        .prepared_request
        .lock()
        .expect("prepared request lock must succeed");
    let private_tmp = PathBuf::from(
        prepared
            .as_ref()
            .and_then(|request| request.command().env().get("TMPDIR"))
            .expect("private TMPDIR must be captured"),
    );
    assert_eq!(
        std::fs::read_to_string(private_tmp.join("marker"))
            .expect("private marker must remain while process handle is alive"),
        "private"
    );
    drop(prepared);
    drop(process);
    assert!(!private_tmp.exists());
}

#[tokio::test]
async fn explicit_denied_root_cannot_be_overridden_by_private_tmp() {
    let root = tempfile::tempdir().expect("temporary workspace must be created");
    let temp_probe = tempfile::tempdir().expect("temporary root probe must be created");
    let denied_tmp_parent = AbsolutePathBuf::new(
        temp_probe
            .path()
            .parent()
            .expect("temporary directory must have a parent")
            .canonicalize()
            .expect("temporary parent must canonicalize"),
    )
    .expect("temporary parent must be absolute");
    let sandbox = sandbox(Arc::new(FakeProvider::available()));
    let mut sandbox_request = request(
        &root,
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::ReadOnly,
        BTreeSet::from([ToolSandboxOperationKind::SpawnProcess]),
    );
    sandbox_request.environment.denied_roots = vec![denied_tmp_parent];
    let ToolSandboxDecision::Allowed(permit) = sandbox
        .evaluate(sandbox_request)
        .expect("request must evaluate")
    else {
        panic!("request must be allowed until execution setup");
    };

    let error = permit
        .spawn(process_command(root.path()))
        .await
        .err()
        .expect("private TMPDIR must not override an explicit denied root");
    assert_eq!(error.category(), TalosSandboxErrorCategory::SetupFailed);
}

#[tokio::test]
async fn full_access_uses_noop_without_constructing_provider_request() {
    let root = tempfile::tempdir().expect("temporary workspace must be created");
    let sandbox =
        ToolSandbox::from_providers(Some(SandboxProviderPlatform::MacosSeatbelt), Vec::new());
    let mut full_request = request(
        &root,
        UserPermissionMode::FullAccess,
        FileSystemPermissionLevel::FullAccess,
        BTreeSet::from([ToolSandboxOperationKind::SpawnProcess]),
    );
    full_request.requested_permissions.network = NetworkSandboxPolicy::Enabled;
    let ToolSandboxDecision::Allowed(permit) = sandbox
        .evaluate(full_request)
        .expect("full access must evaluate")
    else {
        panic!("full access must be allowed");
    };

    let mut process = permit
        .spawn(process_command(root.path()))
        .await
        .expect("Noop command must spawn");
    assert!(process.wait().await.expect("process must wait").success());
    assert_eq!(process.enforcement(), &ToolSandboxEnforcement::None);
}

#[tokio::test]
async fn approved_danger_full_access_uses_noop_in_restricted_user_mode() {
    let root = tempfile::tempdir().expect("temporary workspace must be created");
    let sandbox =
        ToolSandbox::from_providers(Some(SandboxProviderPlatform::MacosSeatbelt), Vec::new());
    let mut full_request = request(
        &root,
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::FullAccess,
        BTreeSet::from([ToolSandboxOperationKind::SpawnProcess]),
    );
    full_request.requested_permissions.network = NetworkSandboxPolicy::Enabled;
    let approval = sandbox.approval_request(&full_request);
    full_request.approval_grant = Some(
        approval
            .allow_once(approval.fingerprint())
            .expect("matching approval should be accepted"),
    );
    let ToolSandboxDecision::Allowed(permit) = sandbox
        .evaluate(full_request)
        .expect("approved full access must evaluate")
    else {
        panic!("approved full access must be allowed");
    };

    let mut process = permit
        .spawn(process_command(root.path()))
        .await
        .expect("approved bypass must spawn through Noop");
    assert!(process.wait().await.expect("process must wait").success());
    assert_eq!(process.enforcement(), &ToolSandboxEnforcement::None);
}

#[cfg(unix)]
#[tokio::test]
async fn prepare_failure_does_not_execute_the_original_command() {
    let root = tempfile::tempdir().expect("temporary workspace must be created");
    let marker = root.path().join("must-not-exist");
    let sandbox = sandbox(Arc::new(FakeProvider::failing_prepare()));
    let ToolSandboxDecision::Allowed(permit) = sandbox
        .evaluate(request(
            &root,
            UserPermissionMode::AskForApproval,
            FileSystemPermissionLevel::ReadOnly,
            BTreeSet::from([ToolSandboxOperationKind::SpawnProcess]),
        ))
        .expect("request must evaluate")
    else {
        panic!("request must be allowed");
    };
    let command = ToolProcessCommand {
        program: "/bin/sh".into(),
        args: vec!["-c".to_string(), format!("touch {}", marker.display())],
        env: BTreeMap::new(),
        cwd: root.path().to_path_buf(),
        stdin: SandboxStdin::Closed,
    };

    let error = permit
        .spawn(command)
        .await
        .err()
        .expect("prepare must fail");
    assert_eq!(
        error,
        TalosSandboxError::SandboxSetupFailed {
            reason: "prepare sentinel".to_string(),
        }
    );
    assert!(!marker.exists());
}

#[test]
fn filesystem_handle_reports_in_process_enforcement() {
    let root = tempfile::tempdir().expect("temporary workspace must be created");
    let sandbox = sandbox(Arc::new(FakeProvider::available()));
    let ToolSandboxDecision::Allowed(permit) = sandbox
        .evaluate(request(
            &root,
            UserPermissionMode::AskForApproval,
            FileSystemPermissionLevel::ReadOnly,
            BTreeSet::from([ToolSandboxOperationKind::FileSystem]),
        ))
        .expect("request must evaluate")
    else {
        panic!("request must be allowed");
    };

    assert_eq!(
        permit
            .file_system()
            .expect("filesystem operation must be granted")
            .enforcement(),
        ToolSandboxEnforcement::InProcessAuthorizer
    );
}

#[test]
fn diagnostics_uses_the_assembly_probe_snapshot() {
    let provider = Arc::new(FakeProvider {
        fail_probe: true,
        ..FakeProvider::available()
    });
    let sandbox = sandbox(provider.clone());

    assert_eq!(provider.probe_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        sandbox.diagnostics(),
        ToolSandboxDiagnostics {
            providers: vec![ToolSandboxProviderDiagnostic {
                platform: SandboxProviderPlatform::MacosSeatbelt,
                provider_name: "fake".to_string(),
                status: ToolSandboxProviderStatus::Failed {
                    reason: "sandbox provider probe failed: probe sentinel".to_string(),
                },
                capabilities: Vec::new(),
            }],
        }
    );
    assert_eq!(provider.probe_count.load(Ordering::SeqCst), 1);
}

#[test]
fn diagnostics_reports_an_unconfigured_host_provider() {
    let sandbox =
        ToolSandbox::from_providers(Some(SandboxProviderPlatform::LinuxBubblewrap), Vec::new());
    assert_eq!(
        sandbox.diagnostics(),
        ToolSandboxDiagnostics {
            providers: vec![ToolSandboxProviderDiagnostic {
                platform: SandboxProviderPlatform::LinuxBubblewrap,
                provider_name: "unconfigured".to_string(),
                status: ToolSandboxProviderStatus::Unsupported {
                    reason: "sandbox provider is not configured".to_string(),
                },
                capabilities: Vec::new(),
            }],
        }
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn diagnostics_preserves_host_provider_assembly_failure() {
    let sandbox = ToolSandbox::with_runtime_paths(SandboxRuntimePaths {
        macos_sandbox_executable: Some(PathBuf::from("relative-sandbox-executable")),
        linux_helper_executable: Some(PathBuf::from("relative-sandbox-helper")),
    });

    let diagnostics = sandbox.diagnostics();
    assert_eq!(diagnostics.providers.len(), 1);
    assert!(matches!(
        diagnostics.providers[0].status,
        ToolSandboxProviderStatus::Failed { .. }
    ));
    assert_ne!(diagnostics.providers[0].provider_name, "unconfigured");
}

#[tokio::test]
async fn unsupported_provider_error_identifies_platform_mode_and_capability() {
    let root = tempfile::tempdir().expect("temporary workspace must be created");
    let sandbox = sandbox(Arc::new(FakeProvider {
        unsupported: true,
        ..FakeProvider::available()
    }));
    let ToolSandboxDecision::Allowed(permit) = sandbox
        .evaluate(request(
            &root,
            UserPermissionMode::AskForApproval,
            FileSystemPermissionLevel::ReadOnly,
            BTreeSet::from([ToolSandboxOperationKind::SpawnProcess]),
        ))
        .expect("request must evaluate")
    else {
        panic!("request must be allowed");
    };

    let error = match permit.spawn(process_command(root.path())).await {
        Ok(_) => panic!("unsupported provider must fail closed"),
        Err(error) => error,
    };
    assert_eq!(
        error.category(),
        TalosSandboxErrorCategory::UnsupportedCapability
    );
    let message = error.to_string();
    assert!(message.contains("MacosSeatbelt"));
    assert!(message.contains("AskForApproval"));
    assert!(message.contains("FileSystemRead"));
}

#[tokio::test]
async fn audit_records_decision_and_actual_process_enforcement() {
    let root = tempfile::tempdir().expect("temporary workspace must be created");
    let sink = Arc::new(RecordingAuditSink::default());
    let sandbox = ToolSandbox::from_providers_with_audit_sink(
        Some(SandboxProviderPlatform::MacosSeatbelt),
        vec![Arc::new(FakeProvider::available())],
        sink.clone(),
    );
    let ToolSandboxDecision::Allowed(permit) = sandbox
        .evaluate(request(
            &root,
            UserPermissionMode::AskForApproval,
            FileSystemPermissionLevel::ReadOnly,
            BTreeSet::from([ToolSandboxOperationKind::SpawnProcess]),
        ))
        .expect("request must evaluate")
    else {
        panic!("request must be allowed");
    };
    let mut process = permit
        .spawn(process_command(root.path()))
        .await
        .expect("provider command must spawn");
    process.wait().await.expect("process must wait");

    let events = sink.events.lock().expect("audit lock must succeed");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].stage, ToolSandboxAuditStage::Evaluate);
    assert_eq!(events[0].outcome, ToolSandboxAuditOutcome::Allowed);
    assert_eq!(events[1].stage, ToolSandboxAuditStage::SpawnProcess);
    assert_eq!(events[1].outcome, ToolSandboxAuditOutcome::Succeeded);
    assert_eq!(
        events[1].enforcement,
        Some(ToolSandboxEnforcement::LocalOs {
            platform: SandboxProviderPlatform::MacosSeatbelt,
            provider_name: "fake".to_string(),
        })
    );
}

#[test]
fn sandbox_errors_expose_stable_categories() {
    assert_eq!(
        TalosSandboxError::ProviderUnavailable {
            platform: SandboxProviderPlatform::LinuxBubblewrap,
        }
        .category(),
        TalosSandboxErrorCategory::ProviderUnavailable
    );
    assert_eq!(
        TalosSandboxError::SandboxBypassDenied.category(),
        TalosSandboxErrorCategory::BypassDenied
    );
    assert_eq!(
        TalosSandboxError::SandboxExecutionFailed {
            reason: "sentinel".to_string(),
        }
        .category(),
        TalosSandboxErrorCategory::ExecutionFailed
    );
}

#[tokio::test]
async fn audit_records_filesystem_denial_with_authorizer_enforcement() {
    let root = tempfile::tempdir().expect("temporary workspace must be created");
    let outside = tempfile::tempdir().expect("outside directory must be created");
    let sink = Arc::new(RecordingAuditSink::default());
    let sandbox = ToolSandbox::from_providers_with_audit_sink(
        Some(SandboxProviderPlatform::MacosSeatbelt),
        vec![Arc::new(FakeProvider::available())],
        sink.clone(),
    );
    let ToolSandboxDecision::Allowed(permit) = sandbox
        .evaluate(request(
            &root,
            UserPermissionMode::AskForApproval,
            FileSystemPermissionLevel::ReadOnly,
            BTreeSet::from([ToolSandboxOperationKind::FileSystem]),
        ))
        .expect("request must evaluate")
    else {
        panic!("request must be allowed");
    };
    let handle = permit
        .file_system()
        .expect("filesystem operation must be granted");
    let error = handle
        .write(&outside.path().join("sentinel"), b"denied")
        .await
        .expect_err("outside write must be denied");
    assert_eq!(
        error.category(),
        TalosSandboxErrorCategory::FileSystemDenied
    );

    let events = sink.events.lock().expect("audit lock must succeed");
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].stage, ToolSandboxAuditStage::FileSystem);
    assert_eq!(events[1].outcome, ToolSandboxAuditOutcome::Failed);
    assert_eq!(
        events[1].enforcement,
        Some(ToolSandboxEnforcement::InProcessAuthorizer)
    );
    assert_eq!(
        events[1].error_category,
        Some(TalosSandboxErrorCategory::FileSystemDenied)
    );
    assert!(!outside.path().join("sentinel").exists());
}

#[cfg(target_os = "macos")]
#[test]
fn default_facade_assembles_the_macos_provider() {
    let diagnostics = ToolSandbox::new().diagnostics();
    assert_eq!(diagnostics.providers.len(), 1);
    assert_eq!(diagnostics.providers[0].provider_name, "macos-seatbelt");
    assert_eq!(
        diagnostics.providers[0].platform,
        SandboxProviderPlatform::MacosSeatbelt
    );
}
