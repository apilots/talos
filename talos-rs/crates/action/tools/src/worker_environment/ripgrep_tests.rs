use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use serde_json::json;
use talos_config::{AgentKernelConfig, ToolSandboxModeConfig, ToolUserPermissionModeConfig};
use talos_tools_sandbox::{SandboxCapability, ToolSandboxPermit};
use talos_types::UserActionResponse;

use super::*;
use crate::{ToolDispatchResult, ToolPending, ToolRegistry};

const FIXTURE_RG: &str = r#"#!/bin/sh
printf '%s\n' '{"type":"match","data":{"path":{"text":"sample.txt"},"lines":{"text":"recovered search"},"line_number":1,"submatches":[{"start":0}]}}'
"#;

#[derive(Clone, Copy)]
enum Outcome {
    Install,
    DownloadFailed,
    NoExecutable,
}

pub(super) struct Fixture {
    attempts: Arc<AtomicUsize>,
    outcome: Outcome,
}

impl Fixture {
    pub(super) async fn run(
        &self,
        root: &Path,
        permit: &ToolSandboxPermit,
    ) -> Result<(), InstallFailure> {
        assert_eq!(
            permit.effective_policy().sandbox,
            SandboxCapability::Enabled
        );
        self.attempts.fetch_add(1, Ordering::SeqCst);
        tokio::task::yield_now().await;
        match self.outcome {
            Outcome::Install => write_fixture(root),
            Outcome::DownloadFailed => return Err(InstallFailure::Download),
            Outcome::NoExecutable => {}
        }
        Ok(())
    }
}

fn write_fixture(root: &Path) {
    let path = root.join("rg");
    std::fs::write(&path, FIXTURE_RG).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

struct Scenario {
    _home: tempfile::TempDir,
    workspace: tempfile::TempDir,
    bin: PathBuf,
    registry: ToolRegistry,
    attempts: Arc<AtomicUsize>,
    config: AgentKernelConfig,
}

impl Scenario {
    fn new(outcome: Outcome) -> Self {
        let home = tempfile::tempdir().unwrap();
        let (root, directory_identity) = private_install_dir(home.path()).unwrap();
        let bin = root.as_path().to_owned();
        let attempts = Arc::new(AtomicUsize::new(0));
        let recovery = Arc::new(RipgrepRecovery {
            root,
            directory_identity,
            worker_name: "fixture-worker".into(),
            instance: uuid::Uuid::new_v4(),
            release: Release {
                target: "fixture",
                sha256: "fixture",
            },
            last_failure: Mutex::new(None),
            fixture: Some(Fixture {
                attempts: Arc::clone(&attempts),
                outcome,
            }),
        });
        let mut worker = WorkerToolEnvironment::assemble(&[], None, None);
        worker.path_with_managed_bin = Some(bin.to_str().unwrap().into());
        worker.ripgrep_recovery = Some(recovery);
        let registry = ToolRegistry::new().with_worker_environment(worker);
        registry.register(Arc::new(crate::GrepTool));
        let mut config = AgentKernelConfig::default();
        config.tools.sandbox_policy.user_permission_mode = ToolUserPermissionModeConfig::FullAccess;
        Self {
            _home: home,
            workspace: tempfile::tempdir().unwrap(),
            bin,
            registry,
            attempts,
            config,
        }
    }

    fn context(&self, denied: Vec<AbsolutePathBuf>) -> ToolContext {
        let root = self.workspace.path().canonicalize().unwrap();
        let environment = ToolSandboxEnvironment::local(
            vec![AbsolutePathBuf::new(root.clone()).unwrap()],
            denied,
        )
        .unwrap();
        ToolContext::new_with_kernel_config("session", "user", Some(Arc::new(self.config.clone())))
            .with_turn_id("turn")
            .with_project_root(root)
            .with_sandbox_environment(environment)
    }

    async fn grep(
        &self,
        call_id: &str,
        context: ToolContext,
    ) -> ToolDispatchResult<ToolDispatchOutcome> {
        let invocation = self
            .registry
            .build_function_invocation(
                call_id,
                "Grep",
                json!({"pattern":"recovered search","path":".","output_mode":"content"}),
            )
            .unwrap();
        self.registry
            .execute_invocation_with_context(invocation, context)
            .await
    }

    async fn approval(&self, call_id: &str, decision: SandboxApprovalDecision) -> ToolContext {
        let context = self.context(Vec::new());
        let pending = self.grep(call_id, context.clone()).await.unwrap();
        let Some(ToolPending::SandboxApproval {
            params,
            request_fingerprint,
        }) = pending.pending()
        else {
            panic!("missing installation approval");
        };
        assert_eq!(params.tool_name, INSTALL_TOOL);
        assert!(matches!(
            &params.presentation,
            SandboxApprovalPresentation::Network { scope } if scope.contains("fixture-worker")
        ));
        context.set_sandbox_approval_fingerprint(call_id, request_fingerprint);
        context.set_user_action_response(call_id, UserActionResponse::SandboxApproval(decision));
        context
    }
}

#[tokio::test]
async fn ripgrep_existing_candidate_runs_without_installing() {
    let fixture = Scenario::new(Outcome::Install);
    write_fixture(&fixture.bin);
    let result = fixture
        .grep("existing", fixture.context(Vec::new()))
        .await
        .unwrap();
    assert!(result
        .payload()
        .unwrap()
        .content()
        .contains("recovered search"));
    assert_eq!(fixture.attempts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn ripgrep_full_access_requires_approval_and_resumes_concurrent_calls() {
    let fixture = Scenario::new(Outcome::Install);
    assert!(!fixture.registry.tool_supports_parallel("Grep"));
    let a = fixture
        .approval("first", SandboxApprovalDecision::AllowOnce)
        .await;
    let b = fixture
        .approval("second", SandboxApprovalDecision::AllowOnce)
        .await;
    assert_eq!(fixture.attempts.load(Ordering::SeqCst), 0);
    let (a, b) = tokio::join!(fixture.grep("first", a), fixture.grep("second", b));
    for result in [a, b] {
        assert!(result
            .unwrap()
            .payload()
            .unwrap()
            .content()
            .contains("recovered search"));
    }
    assert_eq!(fixture.attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ripgrep_installation_denial_does_not_call_installer() {
    let fixture = Scenario::new(Outcome::Install);
    for decision in [
        SandboxApprovalDecision::Deny,
        SandboxApprovalDecision::Cancel,
    ] {
        let context = fixture.approval("declined", decision).await;
        let error = fixture.grep("declined", context).await.unwrap_err();
        assert_eq!(error.error().error_code(), ErrorCode::PermissionDenied);
        assert!(error
            .error()
            .to_api_error()
            .message
            .contains("installation was declined"));
    }
    assert_eq!(fixture.attempts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn ripgrep_approval_is_bound_to_the_executing_worker() {
    let first = Scenario::new(Outcome::Install);
    let second = Scenario::new(Outcome::Install);
    let pending = first
        .approval("moved", SandboxApprovalDecision::AllowOnce)
        .await;
    let context = second.context(Vec::new());
    context.set_sandbox_approval_fingerprint(
        "moved",
        pending.get_sandbox_approval_fingerprint("moved").unwrap(),
    );
    context.set_user_action_response(
        "moved",
        UserActionResponse::SandboxApproval(SandboxApprovalDecision::AllowOnce),
    );
    let error = second.grep("moved", context).await.unwrap_err();
    assert_eq!(error.error().error_code(), ErrorCode::PermissionDenied);
    assert_eq!(second.attempts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn ripgrep_approval_cannot_use_a_replaced_install_directory() {
    let fixture = Scenario::new(Outcome::Install);
    let context = fixture
        .approval("replaced-directory", SandboxApprovalDecision::AllowOnce)
        .await;
    std::fs::rename(&fixture.bin, fixture.bin.with_file_name("original")).unwrap();
    std::os::unix::fs::symlink(fixture.workspace.path(), &fixture.bin).unwrap();
    write_fixture(fixture.workspace.path());
    let error = fixture
        .grep("replaced-directory", context)
        .await
        .unwrap_err();
    assert_eq!(error.error().error_code(), ErrorCode::PermissionDenied);
    assert_eq!(fixture.attempts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn ripgrep_cancelled_approval_does_not_start_an_install() {
    let fixture = Scenario::new(Outcome::Install);
    let context = fixture
        .approval("cancel-run", SandboxApprovalDecision::AllowOnce)
        .await
        .with_cancellation(InvocationCancellation::new(std::future::ready(())));
    let error = fixture.grep("cancel-run", context).await.unwrap_err();
    assert_eq!(error.error().error_code(), ErrorCode::Cancelled);
    assert_eq!(fixture.attempts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn ripgrep_installation_grant_does_not_authorize_search() {
    let mut fixture = Scenario::new(Outcome::Install);
    fixture.config.tools.sandbox_policy.default_mode = ToolSandboxModeConfig::ReadOnly;
    fixture.config.tools.sandbox_policy.user_permission_mode =
        ToolUserPermissionModeConfig::AskForApproval;
    fixture.config.tools.sandbox_policy.network = ToolSandboxNetworkConfig::Enabled;
    let context = fixture
        .approval("two-grants", SandboxApprovalDecision::AllowOnce)
        .await;
    let installer = context
        .get_sandbox_approval_fingerprint("two-grants")
        .unwrap();
    let outcome = fixture.grep("two-grants", context).await.unwrap();
    let Some(ToolPending::SandboxApproval {
        params,
        request_fingerprint,
    }) = outcome.pending()
    else {
        panic!("the original search still requires approval");
    };
    assert_ne!(params.tool_name, INSTALL_TOOL);
    assert_ne!(request_fingerprint, &installer);
    assert_eq!(fixture.attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn ripgrep_failed_or_unusable_install_is_not_retried_by_waiters() {
    for (mode, reason) in [
        (Outcome::DownloadFailed, "release download failed"),
        (
            Outcome::NoExecutable,
            "installed executable could not be verified",
        ),
    ] {
        let fixture = Scenario::new(mode);
        let a = fixture
            .approval("first", SandboxApprovalDecision::AllowOnce)
            .await;
        let b = fixture
            .approval("second", SandboxApprovalDecision::AllowOnce)
            .await;
        let (a, b) = tokio::join!(fixture.grep("first", a), fixture.grep("second", b));
        for result in [a, b] {
            let error = result.unwrap_err();
            assert_eq!(error.error().error_code(), ErrorCode::ToolExecutionFailed);
            assert!(error.error().to_api_error().message.contains(reason));
        }
        assert_eq!(fixture.attempts.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn ripgrep_sandbox_denial_does_not_offer_installation() {
    let mut fixture = Scenario::new(Outcome::Install);
    fixture.config.tools.sandbox_policy.user_permission_mode =
        ToolUserPermissionModeConfig::AskForApproval;
    let workspace = AbsolutePathBuf::new(fixture.workspace.path().canonicalize().unwrap()).unwrap();
    let error = fixture
        .grep("denied-path", fixture.context(vec![workspace]))
        .await
        .unwrap_err();
    assert_eq!(error.error().error_code(), ErrorCode::PermissionDenied);
    assert_eq!(fixture.attempts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn ripgrep_restricted_network_uses_manual_recovery() {
    let mut fixture = Scenario::new(Outcome::Install);
    fixture.config.tools.sandbox_policy.network = ToolSandboxNetworkConfig::Restricted;
    let error = fixture
        .grep("no-route", fixture.context(Vec::new()))
        .await
        .unwrap_err();
    assert_eq!(error.error().error_code(), ErrorCode::ResourceNotFound);
    assert!(error
        .error()
        .to_api_error()
        .message
        .contains("agent.kernel.tools.environment.path_prepend"));
}
