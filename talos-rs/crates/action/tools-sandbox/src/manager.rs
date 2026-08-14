use std::fs;
use std::path::Path;
use std::sync::Arc;

use talos_sandbox_spi::{
    AbsolutePathBuf, FileSystemIsolationSpec, NetworkIsolationSpec, PreparedSandboxCommand,
    SandboxExecutionCommand, SandboxExecutionRequest, SandboxIsolationCapability, SandboxProvider,
    SandboxProviderPlatform, SandboxProviderSnapshot, SandboxRuntimeRequirements,
};
use talos_types::UserPermissionMode;
use tempfile::TempDir;

use crate::facade::{
    map_provider_error, TalosSandboxError, ToolSandboxDiagnostics, ToolSandboxEnforcement,
    ToolSandboxProviderDiagnostic, ToolSandboxProviderStatus,
};
use crate::process::ToolProcessCommand;
use crate::{
    EffectiveToolPermissionPolicy, FileSystemSandboxPolicy, NetworkSandboxPolicy,
    SandboxCapability, ToolInvocationIdentity,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LocalSandboxTarget {
    Noop,
    Provider(SandboxProviderPlatform),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ToolSandboxContext {
    pub(crate) invocation: ToolInvocationIdentity,
    pub(crate) effective_policy: EffectiveToolPermissionPolicy,
    pub(crate) user_mode: UserPermissionMode,
    pub(crate) target: LocalSandboxTarget,
}

impl ToolSandboxContext {
    pub(crate) fn new(
        invocation: ToolInvocationIdentity,
        effective_policy: EffectiveToolPermissionPolicy,
        user_mode: UserPermissionMode,
        platform: Option<SandboxProviderPlatform>,
    ) -> Result<Self, TalosSandboxError> {
        let target = match effective_policy.sandbox {
            SandboxCapability::Disabled => LocalSandboxTarget::Noop,
            SandboxCapability::Enabled => LocalSandboxTarget::Provider(
                platform.ok_or(TalosSandboxError::UnsupportedPlatform)?,
            ),
            SandboxCapability::External => {
                return Err(TalosSandboxError::InvalidLocalExecutionContext);
            }
        };
        if matches!(target, LocalSandboxTarget::Noop)
            && effective_policy.sandbox != SandboxCapability::Disabled
        {
            return Err(TalosSandboxError::InvalidLocalExecutionContext);
        }
        Ok(Self {
            invocation,
            effective_policy,
            user_mode,
            target,
        })
    }

    pub(crate) fn is_noop(&self) -> bool {
        matches!(self.target, LocalSandboxTarget::Noop)
    }
}

struct SandboxProviderRegistration {
    provider: Arc<dyn SandboxProvider>,
    platform: SandboxProviderPlatform,
    snapshot: SandboxProviderSnapshot,
}

pub(crate) struct SandboxManager {
    platform: Option<SandboxProviderPlatform>,
    providers: Vec<SandboxProviderRegistration>,
    unavailable: Option<ToolSandboxProviderDiagnostic>,
}

impl std::fmt::Debug for SandboxManager {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SandboxManager")
            .field("platform", &self.platform)
            .field("provider_count", &self.providers.len())
            .finish()
    }
}

impl SandboxManager {
    #[cfg(test)]
    pub(crate) fn new(
        platform: Option<SandboxProviderPlatform>,
        providers: Vec<Arc<dyn SandboxProvider>>,
    ) -> Self {
        Self::new_with_unavailable(platform, providers, None)
    }

    pub(crate) fn new_with_unavailable(
        platform: Option<SandboxProviderPlatform>,
        providers: Vec<Arc<dyn SandboxProvider>>,
        unavailable: Option<ToolSandboxProviderDiagnostic>,
    ) -> Self {
        let providers = match platform {
            Some(platform) => providers
                .into_iter()
                .filter(|provider| provider.supports_platform(platform))
                .map(|provider| {
                    let snapshot = match provider.probe(platform) {
                        Ok(capabilities) => SandboxProviderSnapshot::from(capabilities),
                        Err(error) => SandboxProviderSnapshot::Failed {
                            reason: error.to_string(),
                        },
                    };
                    SandboxProviderRegistration {
                        provider,
                        platform,
                        snapshot,
                    }
                })
                .collect(),
            None => Vec::new(),
        };
        Self {
            platform,
            providers,
            unavailable,
        }
    }

    pub(crate) fn platform(&self) -> Option<SandboxProviderPlatform> {
        self.platform
    }

    pub(crate) fn prepare_command(
        &self,
        command: ToolProcessCommand,
        context: &ToolSandboxContext,
    ) -> Result<PreparedToolCommand, TalosSandboxError> {
        match context.target {
            LocalSandboxTarget::Noop => NoopBackend::prepare(command, context),
            LocalSandboxTarget::Provider(platform) => {
                let compiled = SandboxExecutionCompiler::compile(command, context, platform)?;
                let registration = self
                    .providers
                    .iter()
                    .find(|registration| registration.platform == platform)
                    .ok_or(TalosSandboxError::ProviderUnavailable { platform })?;
                registration
                    .provider
                    .support(&compiled.request, &registration.snapshot)
                    .require_supported()
                    .map_err(|error| map_provider_execution_error(error, platform, context))?;
                let prepared = registration
                    .provider
                    .prepare(compiled.request)
                    .map_err(|error| map_provider_execution_error(error, platform, context))?;
                Ok(PreparedToolCommand::local_os(
                    prepared,
                    platform,
                    registration.provider.name(),
                    compiled.private_tmp,
                ))
            }
        }
    }

    pub(crate) fn diagnostics(&self) -> ToolSandboxDiagnostics {
        if self.providers.is_empty() {
            if let Some(unavailable) = &self.unavailable {
                return ToolSandboxDiagnostics {
                    providers: vec![unavailable.clone()],
                };
            }
            return ToolSandboxDiagnostics {
                providers: self
                    .platform
                    .map(|platform| ToolSandboxProviderDiagnostic {
                        platform,
                        provider_name: "unconfigured".to_string(),
                        status: ToolSandboxProviderStatus::Unsupported {
                            reason: "sandbox provider is not configured".to_string(),
                        },
                        capabilities: Vec::new(),
                    })
                    .into_iter()
                    .collect(),
            };
        }
        ToolSandboxDiagnostics {
            providers: self
                .providers
                .iter()
                .map(|registration| {
                    let capabilities = snapshot_capabilities(&registration.snapshot);
                    let status = match &registration.snapshot {
                        SandboxProviderSnapshot::Available { .. } if capabilities.is_empty() => {
                            ToolSandboxProviderStatus::Unsupported {
                                reason: "provider exposes no enforcing capabilities".to_string(),
                            }
                        }
                        SandboxProviderSnapshot::Available { .. } => {
                            ToolSandboxProviderStatus::Available
                        }
                        SandboxProviderSnapshot::Failed { reason } => {
                            ToolSandboxProviderStatus::Failed {
                                reason: reason.clone(),
                            }
                        }
                    };
                    ToolSandboxProviderDiagnostic {
                        platform: registration.platform,
                        provider_name: registration.provider.name().to_string(),
                        status,
                        capabilities,
                    }
                })
                .collect(),
        }
    }
}

fn map_provider_execution_error(
    error: talos_sandbox_spi::SandboxProviderError,
    platform: SandboxProviderPlatform,
    context: &ToolSandboxContext,
) -> TalosSandboxError {
    match error {
        talos_sandbox_spi::SandboxProviderError::UnsupportedCapability { capability, reason } => {
            TalosSandboxError::UnsupportedCapability {
                capability,
                reason: format!(
                    "requested platform {platform:?}, requested mode {:?}: {reason}",
                    context.user_mode
                ),
            }
        }
        other => map_provider_error(other),
    }
}

pub(crate) struct PreparedToolCommand {
    pub(crate) command: SandboxExecutionCommand,
    pub(crate) enforcement: ToolSandboxEnforcement,
    pub(crate) private_tmp: Option<TempDir>,
}

impl PreparedToolCommand {
    fn local_os(
        prepared: PreparedSandboxCommand,
        platform: SandboxProviderPlatform,
        provider_name: &str,
        private_tmp: TempDir,
    ) -> Self {
        Self {
            command: prepared.into_command(),
            enforcement: ToolSandboxEnforcement::LocalOs {
                platform,
                provider_name: provider_name.to_string(),
            },
            private_tmp: Some(private_tmp),
        }
    }
}

struct NoopBackend;

impl NoopBackend {
    fn prepare(
        command: ToolProcessCommand,
        context: &ToolSandboxContext,
    ) -> Result<PreparedToolCommand, TalosSandboxError> {
        if !context.is_noop() {
            return Err(TalosSandboxError::SandboxBypassDenied);
        }
        Ok(PreparedToolCommand {
            command: compile_command(command)?,
            enforcement: ToolSandboxEnforcement::None,
            private_tmp: None,
        })
    }
}

struct SandboxExecutionCompiler;

struct CompiledSandboxExecution {
    request: SandboxExecutionRequest,
    private_tmp: TempDir,
}

impl SandboxExecutionCompiler {
    fn compile(
        mut command: ToolProcessCommand,
        context: &ToolSandboxContext,
        platform: SandboxProviderPlatform,
    ) -> Result<CompiledSandboxExecution, TalosSandboxError> {
        if context.effective_policy.sandbox != SandboxCapability::Enabled {
            return Err(TalosSandboxError::InvalidLocalExecutionContext);
        }
        command
            .env
            .retain(|name, _| allowed_sandbox_environment(name));
        let private_tmp = tempfile::Builder::new()
            .prefix("talos-sandbox-")
            .tempdir()
            .map_err(|error| TalosSandboxError::SandboxSetupFailed {
                reason: format!("failed to create private temporary directory: {error}"),
            })?;
        let private_home_path = private_tmp.path().join("home");
        let private_tmp_path = private_tmp.path().join("tmp");
        fs::create_dir_all(&private_home_path).map_err(|error| {
            TalosSandboxError::SandboxSetupFailed {
                reason: format!("failed to create private home directory: {error}"),
            }
        })?;
        fs::create_dir_all(&private_tmp_path).map_err(|error| {
            TalosSandboxError::SandboxSetupFailed {
                reason: format!("failed to create private temporary directory: {error}"),
            }
        })?;
        let private_home_root = canonical_existing_directory(&private_home_path)?;
        let private_tmp_root = canonical_existing_directory(&private_tmp_path)?;
        command.env.insert(
            "HOME".to_string(),
            private_home_root.as_path().to_string_lossy().into_owned(),
        );
        command.env.insert(
            "TMPDIR".to_string(),
            private_tmp_root.as_path().to_string_lossy().into_owned(),
        );
        if platform == SandboxProviderPlatform::MacosSeatbelt {
            command
                .env
                .insert("xcrun_nocache".to_string(), "1".to_string());
        }
        let command = compile_command(command)?;
        let runtime_writable_roots = vec![private_home_root, private_tmp_root];
        let file_system = compile_file_system(
            &context.effective_policy,
            command.cwd(),
            &runtime_writable_roots,
        )?;
        let network = match context.effective_policy.permissions.network {
            NetworkSandboxPolicy::Restricted => NetworkIsolationSpec::Restricted,
            NetworkSandboxPolicy::Enabled => NetworkIsolationSpec::Enabled,
        };
        let request = SandboxExecutionRequest::new(
            platform,
            command,
            file_system,
            network,
            SandboxRuntimeRequirements { private_tmp: true },
        )
        .map_err(map_provider_error)?;
        Ok(CompiledSandboxExecution {
            request,
            private_tmp,
        })
    }
}

fn allowed_sandbox_environment(name: &str) -> bool {
    matches!(
        name,
        "PATH"
            | "HOME"
            | "USER"
            | "LOGNAME"
            | "SHELL"
            | "TMPDIR"
            | "LANG"
            | "TERM"
            | "COLORTERM"
            | "xcrun_nocache"
    ) || name.starts_with("LC_")
}

fn compile_command(
    command: ToolProcessCommand,
) -> Result<SandboxExecutionCommand, TalosSandboxError> {
    let program = canonical_existing_file(&command.program)?;
    let cwd = canonical_existing_directory(&command.cwd)?;
    SandboxExecutionCommand::new(program, command.args, command.env, cwd, command.stdin)
        .map_err(map_provider_error)
}

fn compile_file_system(
    policy: &EffectiveToolPermissionPolicy,
    cwd: &AbsolutePathBuf,
    runtime_writable_roots: &[AbsolutePathBuf],
) -> Result<FileSystemIsolationSpec, TalosSandboxError> {
    let (readable, mut writable, denied) = match &policy.permissions.file_system {
        FileSystemSandboxPolicy::ReadOnly(scope) => (
            canonical_roots(&scope.readable_roots)?,
            Vec::new(),
            normalize_denied_roots(&scope.denied_roots)?,
        ),
        FileSystemSandboxPolicy::WorkspaceWrite(scope) => (
            canonical_roots(&scope.readable_roots)?,
            canonical_roots(&scope.writable_roots)?,
            normalize_denied_roots(&scope.denied_roots)?,
        ),
        FileSystemSandboxPolicy::DangerFullAccess => {
            let root = filesystem_root(cwd)?;
            (vec![root.clone()], vec![root], Vec::new())
        }
    };
    writable.extend(runtime_writable_roots.iter().cloned());
    let file_system =
        FileSystemIsolationSpec::new(readable, writable, denied).map_err(map_provider_error)?;
    if runtime_writable_roots
        .iter()
        .any(|root| file_system.access_for(root) != talos_sandbox_spi::SandboxPathAccess::Writable)
    {
        return Err(TalosSandboxError::SandboxSetupFailed {
            reason: "private runtime directory conflicts with denied filesystem roots".to_string(),
        });
    }
    Ok(file_system)
}

fn canonical_roots(roots: &[AbsolutePathBuf]) -> Result<Vec<AbsolutePathBuf>, TalosSandboxError> {
    roots
        .iter()
        .map(|root| canonical_existing_directory(root.as_path()))
        .collect()
}

fn normalize_denied_roots(
    roots: &[AbsolutePathBuf],
) -> Result<Vec<AbsolutePathBuf>, TalosSandboxError> {
    roots
        .iter()
        .map(|root| canonicalize_nearest_existing(root.as_path()))
        .collect()
}

fn canonical_existing_file(path: &Path) -> Result<AbsolutePathBuf, TalosSandboxError> {
    let canonical = fs::canonicalize(path).map_err(setup_error(path))?;
    if !canonical.is_file() {
        return Err(TalosSandboxError::SandboxSetupFailed {
            reason: format!("program is not a file: {}", path.display()),
        });
    }
    AbsolutePathBuf::new(canonical).map_err(map_provider_error)
}

fn canonical_existing_directory(path: &Path) -> Result<AbsolutePathBuf, TalosSandboxError> {
    let canonical = fs::canonicalize(path).map_err(setup_error(path))?;
    if !canonical.is_dir() {
        return Err(TalosSandboxError::SandboxSetupFailed {
            reason: format!("path is not a directory: {}", path.display()),
        });
    }
    AbsolutePathBuf::new(canonical).map_err(map_provider_error)
}

fn canonicalize_nearest_existing(path: &Path) -> Result<AbsolutePathBuf, TalosSandboxError> {
    let mut current = path;
    let mut suffix = Vec::new();
    while !current.exists() {
        let name = current
            .file_name()
            .ok_or_else(|| TalosSandboxError::SandboxSetupFailed {
                reason: format!("path has no existing ancestor: {}", path.display()),
            })?;
        suffix.push(name.to_owned());
        current = current
            .parent()
            .ok_or_else(|| TalosSandboxError::SandboxSetupFailed {
                reason: format!("path has no existing ancestor: {}", path.display()),
            })?;
    }
    let mut normalized = fs::canonicalize(current).map_err(setup_error(current))?;
    for component in suffix.into_iter().rev() {
        normalized.push(component);
    }
    AbsolutePathBuf::new(normalized).map_err(map_provider_error)
}

fn filesystem_root(path: &AbsolutePathBuf) -> Result<AbsolutePathBuf, TalosSandboxError> {
    let root = path
        .as_path()
        .ancestors()
        .last()
        .ok_or(TalosSandboxError::InvalidLocalExecutionContext)?;
    AbsolutePathBuf::new(root.to_path_buf()).map_err(map_provider_error)
}

fn setup_error(path: &Path) -> impl FnOnce(std::io::Error) -> TalosSandboxError + '_ {
    move |error| TalosSandboxError::SandboxSetupFailed {
        reason: format!("failed to resolve {}: {error}", path.display()),
    }
}

fn snapshot_capabilities(snapshot: &SandboxProviderSnapshot) -> Vec<SandboxIsolationCapability> {
    let SandboxProviderSnapshot::Available { capabilities } = snapshot else {
        return Vec::new();
    };
    let mut result = Vec::new();
    if capabilities.file_system.read {
        result.push(SandboxIsolationCapability::FileSystemRead);
    }
    if capabilities.file_system.write {
        result.push(SandboxIsolationCapability::FileSystemWrite);
    }
    if capabilities.file_system.deny_read {
        result.push(SandboxIsolationCapability::DenyRead);
    }
    if capabilities.network.restricted {
        result.push(SandboxIsolationCapability::NetworkRestricted);
    }
    if capabilities.private_tmp {
        result.push(SandboxIsolationCapability::PrivateTmp);
    }
    result
}

pub(crate) fn host_platform() -> Option<SandboxProviderPlatform> {
    #[cfg(target_os = "macos")]
    return Some(SandboxProviderPlatform::MacosSeatbelt);
    #[cfg(target_os = "linux")]
    return Some(SandboxProviderPlatform::LinuxBubblewrap);
    #[cfg(target_os = "windows")]
    return Some(SandboxProviderPlatform::WindowsRestrictedToken);
    #[allow(unreachable_code)]
    None
}
