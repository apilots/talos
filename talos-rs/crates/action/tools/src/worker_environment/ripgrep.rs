use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use talos_config::ToolSandboxNetworkConfig;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_tools_sandbox::{
    AbsolutePathBuf, FileSystemOperation, FileSystemPermissionLevel, NetworkSandboxPolicy,
    RequestedFileSystemAccess, RequestedToolPermissions, SandboxStdin, ToolEscalationPolicy,
    ToolGovernanceConstraints, ToolInvocationIdentity, ToolProcessCommand,
    ToolProcessTerminationTarget, ToolSandbox, ToolSandboxDecision, ToolSandboxEnvironment,
    ToolSandboxOperationKind, ToolSandboxProviderStatus, ToolSandboxRequest,
};
use talos_types::{
    SandboxApprovalDecision, SandboxApprovalParams, SandboxApprovalPresentation,
    SandboxApprovalReason, UserPermissionMode,
};
use tokio::sync::Mutex;

use super::{is_executable, ToolEnvironmentConfig};
use crate::context::wait_for_cancellation;
use crate::invocation::{ToolCallPayload, ToolInvocation};
use crate::runtimes::fs_search;
use crate::{InvocationCancellation, ToolContext, ToolDispatchOutcome};

const INSTALLER: &str = include_str!("ripgrep_install.sh");
const VERSION: &str = "15.1.0";
const INSTALL_TOOL: &str = "Install ripgrep for Grep";
const MANUAL: &str = "Install ripgrep on the executing Worker, or add its executable directory to agent.kernel.tools.environment.path_prepend, then restart the Worker.";

struct Release {
    target: &'static str,
    sha256: &'static str,
}

impl Release {
    fn current() -> Option<Self> {
        // Digests from the upstream release assets, verified before extraction.
        // https://github.com/BurntSushi/ripgrep/releases/tag/15.1.0
        let (target, sha256) = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => (
                "aarch64-apple-darwin",
                "378e973289176ca0c6054054ee7f631a065874a352bf43f0fa60ef079b6ba715",
            ),
            ("macos", "x86_64") => (
                "x86_64-apple-darwin",
                "64811cb24e77cac3057d6c40b63ac9becf9082eedd54ca411b475b755d334882",
            ),
            ("linux", "aarch64") if cfg!(target_env = "gnu") => (
                "aarch64-unknown-linux-gnu",
                "2b661c6ef508e902f388e9098d9c4c5aca72c87b55922d94abdba830b4dc885e",
            ),
            ("linux", "x86_64") => (
                "x86_64-unknown-linux-musl",
                "1c9297be4a084eea7ecaedf93eb03d058d6faae29bbc57ecdaf5063921491599",
            ),
            _ => return None,
        };
        Some(Self { target, sha256 })
    }

    fn archive_stem(&self) -> String {
        format!("ripgrep-{VERSION}-{}", self.target)
    }

    fn url(&self) -> String {
        format!(
            "https://github.com/BurntSushi/ripgrep/releases/download/{VERSION}/{}.tar.gz",
            self.archive_stem()
        )
    }
}

pub(crate) struct RipgrepRecovery {
    root: AbsolutePathBuf,
    directory_identity: String,
    worker_name: String,
    instance: uuid::Uuid,
    release: Release,
    last_failure: Mutex<Option<InstallFailure>>,
    #[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
    fixture: Option<tests::Fixture>,
}

impl RipgrepRecovery {
    pub(super) fn new(home: &Path, hostname: Option<&str>) -> Option<Self> {
        let release = Release::current()?;
        let (root, directory_identity) = match private_install_dir(home) {
            Ok(directory) => directory,
            Err(_) => {
                tracing::warn!("Worker ripgrep recovery directory is unavailable");
                return None;
            }
        };
        let worker_name = hostname
            .map(|name| {
                name.chars()
                    .filter(|ch| !ch.is_control())
                    .take(128)
                    .collect()
            })
            .filter(|name: &String| !name.trim().is_empty())
            .unwrap_or_else(|| "the selected Worker".to_string());
        Some(Self {
            root,
            directory_identity,
            worker_name,
            instance: uuid::Uuid::new_v4(),
            release,
            last_failure: Mutex::new(None),
            #[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
            fixture: None,
        })
    }

    pub(super) fn root(&self) -> &Path {
        self.root.as_path()
    }

    pub(crate) fn directory_is_current(&self) -> bool {
        directory_identity(self.root()).is_ok_and(|current| current == self.directory_identity)
    }

    pub(crate) fn installer_available(&self, sandbox: &ToolSandbox) -> bool {
        #[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
        if self.fixture.is_some() {
            return true;
        }
        sandbox
            .diagnostics()
            .providers
            .iter()
            .any(|provider| provider.status == ToolSandboxProviderStatus::Available)
            && required_programs()
                .iter()
                .all(|program| is_executable(Path::new(program)))
    }

    pub(crate) fn request(
        &self,
        invocation: &ToolInvocation,
        context: &ToolContext,
    ) -> Option<ToolSandboxRequest> {
        let original = context.sandbox_environment()?;
        if !self.directory_is_current() {
            return None;
        }
        if context.kernel_config().is_some_and(|config| {
            config.tools.sandbox_policy.network == ToolSandboxNetworkConfig::Restricted
        }) || original
            .workspace_roots
            .iter()
            .chain(&original.denied_roots)
            .any(|root| {
                self.root().starts_with(root.as_path()) || root.as_path().starts_with(self.root())
            })
            || original.location != talos_tools_sandbox::ToolExecutionLocation::Local
        {
            return None;
        }
        let environment =
            ToolSandboxEnvironment::local(vec![self.root.clone()], original.denied_roots.clone())
                .ok()?;
        let access =
            RequestedFileSystemAccess::new(FileSystemOperation::Write, vec![self.root.clone()])
                .ok()?;
        let permissions = RequestedToolPermissions::new(
            FileSystemPermissionLevel::WorkspaceWrite,
            NetworkSandboxPolicy::Enabled,
            None,
            BTreeSet::from([ToolSandboxOperationKind::SpawnProcess]),
        )
        .and_then(|permissions| permissions.with_file_system_access(vec![access]))
        .ok()?;
        let scope = serde_json::to_vec(&(
            self.instance,
            &self.directory_identity,
            self.release.url(),
            self.release.sha256,
            context.session_id(),
            context.user_id(),
            context.turn_id(),
            &invocation.payload,
        ))
        .ok()?;
        Some(ToolSandboxRequest {
            invocation: ToolInvocationIdentity::new(&invocation.call_id, INSTALL_TOOL)
                .with_scope_digest(crate::state::sha256_hex(&scope)),
            user_mode: UserPermissionMode::AskForApproval,
            governance: ToolGovernanceConstraints {
                maximum_file_system: Some(FileSystemPermissionLevel::WorkspaceWrite),
                maximum_network: Some(NetworkSandboxPolicy::Enabled),
                escalation: ToolEscalationPolicy::RequiresApproval,
            },
            requested_permissions: permissions,
            approval_grant: None,
            command_risk_assessment: None,
            environment,
        })
    }

    pub(crate) async fn recover(
        &self,
        request: ToolSandboxRequest,
        replay: Option<SandboxApprovalDecision>,
        invocation: &ToolInvocation,
        context: &ToolContext,
        sandbox: &ToolSandbox,
    ) -> TalosResult<Option<ToolDispatchOutcome>> {
        if matches!(
            replay,
            Some(SandboxApprovalDecision::Deny | SandboxApprovalDecision::Cancel)
        ) {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                format!("Ripgrep installation was declined. {MANUAL}"),
            ));
        }
        if context.tool_search_path().resolve("rg").is_some() {
            return Ok(None);
        }
        let ToolCallPayload::Function { arguments } = &invocation.payload else {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Grep requires function arguments.",
            ));
        };
        let input = crate::decode_tool_input(arguments.clone(), "Grep")?;
        let path = fs_search::search_path(&input, context)?;
        if request
            .environment
            .denied_roots
            .iter()
            .any(|root| path.starts_with(root.as_path()))
        {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "The sandbox denies access to the requested search path.",
            ));
        }

        // Serialize the attempt across Sessions. Waiters re-resolve PATH and
        // observe the same failure if the approved attempt did not succeed.
        let cancellation = context.cancellation();
        let mut last_failure = tokio::select! {
            biased;
            () = wait_for_cancellation(cancellation.clone()) => return Err(InstallFailure::Cancelled.into_error()),
            state = self.last_failure.lock() => state,
        };
        if context.tool_search_path().resolve("rg").is_some() {
            return Ok(None);
        }
        if let Some(failure) = *last_failure {
            return Err(failure.into_error());
        }
        if replay.is_none() {
            let Ok(ToolSandboxDecision::RequiresApproval(approval)) = sandbox.evaluate(request)
            else {
                return Err(InstallFailure::Sandbox.into_error());
            };
            return Ok(Some(ToolDispatchOutcome::pending_sandbox_approval(
                invocation.tool_name.clone(),
                SandboxApprovalParams {
                    tool_name: INSTALL_TOOL.into(),
                    reason: SandboxApprovalReason::NetworkAccess,
                    presentation: SandboxApprovalPresentation::Network {
                        scope: format!(
                            "Download the verified ripgrep {VERSION} release from GitHub and install it in the private Talos tool directory on {}. Then continue the original Grep call.",
                            self.worker_name
                        ),
                    },
                    available_decisions: vec![
                        SandboxApprovalDecision::AllowOnce,
                        SandboxApprovalDecision::Deny,
                        SandboxApprovalDecision::Cancel,
                    ],
                },
                approval.fingerprint().to_owned(),
            )));
        }
        let Ok(ToolSandboxDecision::Allowed(permit)) = sandbox.evaluate(request) else {
            return Err(InstallFailure::Sandbox.into_error());
        };
        *last_failure = Some(InstallFailure::Interrupted);
        let result = self.install(&permit, cancellation).await.and_then(|()| {
            context
                .tool_search_path()
                .resolve("rg")
                .is_some()
                .then_some(())
                .ok_or(InstallFailure::Executable)
        });
        *last_failure = result.err().map(|failure| match failure {
            InstallFailure::Cancelled => InstallFailure::Interrupted,
            other => other,
        });
        result.map(|()| None).map_err(InstallFailure::into_error)
    }

    async fn install(
        &self,
        permit: &talos_tools_sandbox::ToolSandboxPermit,
        cancellation: Option<InvocationCancellation>,
    ) -> Result<(), InstallFailure> {
        if !self.directory_is_current() {
            return Err(InstallFailure::Storage);
        }
        #[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
        if let Some(fixture) = &self.fixture {
            return fixture.run(self.root(), permit).await;
        }
        let command = ToolProcessCommand {
            program: PathBuf::from("/bin/sh"),
            args: vec![
                "-c".into(),
                INSTALLER.into(),
                "talos-ripgrep-install".into(),
                self.release.url(),
                self.release.sha256.into(),
                format!("{}/rg", self.release.archive_stem()),
                hash_program().into(),
                self.directory_identity.clone(),
            ],
            env: [
                ("LC_ALL".into(), "C".into()),
                ("PATH".into(), "/usr/bin:/bin".into()),
            ]
            .into(),
            cwd: self.root().to_owned(),
            stdin: SandboxStdin::Closed,
        };
        let mut child = tokio::select! {
            biased;
            () = wait_for_cancellation(cancellation.clone()) => return Err(InstallFailure::Cancelled),
            child = permit.spawn(command) => child.map_err(|_| InstallFailure::Sandbox)?,
        };
        let mut cleanup = InstallProcess(child.termination_target());
        let result = tokio::select! {
            biased;
            () = wait_for_cancellation(cancellation) => Err(InstallFailure::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(60), child.wait()) => match result {
                Ok(result) => result.map_err(|_| InstallFailure::Sandbox),
                Err(_) => Err(InstallFailure::Timeout),
            },
        };
        let status = match result {
            Ok(status) => {
                cleanup.0.take();
                status
            }
            Err(failure) => {
                drop(cleanup);
                let _ = tokio::time::timeout(Duration::from_millis(250), child.wait()).await;
                return Err(failure);
            }
        };
        match status.code() {
            Some(0) => Ok(()),
            Some(20) => Err(InstallFailure::Download),
            Some(21) => Err(InstallFailure::Integrity),
            Some(22) => Err(InstallFailure::Unpack),
            Some(23) => Err(InstallFailure::Executable),
            Some(24) => Err(InstallFailure::Storage),
            _ => Err(InstallFailure::Sandbox),
        }
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[path = "ripgrep_tests.rs"]
mod tests;

fn hash_program() -> &'static str {
    if cfg!(target_os = "macos") {
        "/usr/bin/shasum"
    } else {
        "/usr/bin/sha256sum"
    }
}

fn required_programs() -> [&'static str; 10] {
    [
        "/bin/sh",
        "/usr/bin/curl",
        "/usr/bin/tar",
        "/usr/bin/mktemp",
        "/usr/bin/stat",
        "/bin/chmod",
        "/bin/mv",
        "/bin/rm",
        "/bin/rmdir",
        hash_program(),
    ]
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn private_install_dir(home: &Path) -> std::io::Result<(AbsolutePathBuf, String)> {
    use std::os::unix::fs::DirBuilderExt;

    let root = home.canonicalize()?.join("tool-bin");
    match std::fs::DirBuilder::new().mode(0o700).create(&root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let identity = directory_identity(&root)?;
    if !ToolEnvironmentConfig::is_valid_path_entry(&root) {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
    }
    let root = AbsolutePathBuf::new(root).map_err(std::io::Error::other)?;
    Ok((root, identity))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn directory_identity(path: &Path) -> std::io::Result<String> {
    use std::os::unix::fs::MetadataExt;

    let metadata = path.symlink_metadata()?;
    // SAFETY: geteuid has no preconditions.
    if !metadata.is_dir()
        || metadata.mode() & 0o077 != 0
        || metadata.uid() != unsafe { libc::geteuid() }
        || path.canonicalize()? != path
    {
        return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    }
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn private_install_dir(_: &Path) -> std::io::Result<(AbsolutePathBuf, String)> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn directory_identity(_: &Path) -> std::io::Result<String> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

struct InstallProcess(Option<ToolProcessTerminationTarget>);

impl Drop for InstallProcess {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(target) = self.0 {
            let _ = target.kill();
        }
    }
}

#[derive(Clone, Copy)]
enum InstallFailure {
    Sandbox,
    Download,
    Integrity,
    Unpack,
    Executable,
    Storage,
    Timeout,
    Cancelled,
    Interrupted,
}

impl InstallFailure {
    fn into_error(self) -> TalosError {
        let reason = match self {
            Self::Cancelled => {
                return TalosError::new(ErrorCode::Cancelled, "Ripgrep installation was cancelled.")
            }
            Self::Sandbox => "the Worker sandbox could not run the installer",
            Self::Download => "the release download failed",
            Self::Integrity => "the downloaded release failed integrity verification",
            Self::Unpack => "the verified release could not be unpacked",
            Self::Executable => "the installed executable could not be verified",
            Self::Storage => "the Worker could not publish the executable",
            Self::Timeout => "the installer timed out",
            Self::Interrupted => "the installer was interrupted",
        };
        TalosError::new(
            ErrorCode::ToolExecutionFailed,
            format!("Ripgrep recovery failed because {reason}. {MANUAL}"),
        )
    }
}
