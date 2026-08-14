use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;

use crate::manager::{host_platform, SandboxManager, ToolSandboxContext};
use crate::permission::EffectiveToolPermissionPolicyResolver;
use crate::{
    ExternalSandboxEnforcement, SandboxIsolationCapability, SandboxProviderPlatform,
    ToolPermissionError, ToolSandboxAuditEvent, ToolSandboxAuditOutcome, ToolSandboxAuditSink,
    ToolSandboxAuditStage, ToolSandboxDecision, ToolSandboxRequest,
};
use talos_sandbox_spi::{SandboxProvider, SandboxProviderError};

#[derive(Clone)]
pub(crate) struct ToolSandboxExecution {
    manager: Arc<SandboxManager>,
    audit_sink: Arc<dyn ToolSandboxAuditSink>,
}

impl fmt::Debug for ToolSandboxExecution {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ToolSandboxExecution")
    }
}

impl ToolSandboxExecution {
    pub(crate) fn new(manager: SandboxManager, audit_sink: Arc<dyn ToolSandboxAuditSink>) -> Self {
        Self {
            manager: Arc::new(manager),
            audit_sink,
        }
    }

    pub(crate) fn manager(&self) -> &SandboxManager {
        &self.manager
    }

    pub(crate) fn record(&self, event: &ToolSandboxAuditEvent) {
        self.audit_sink.record(event);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ToolSandboxPermitBinding {
    pub(crate) context: Arc<ToolSandboxContext>,
    pub(crate) execution: Arc<ToolSandboxExecution>,
}

impl PartialEq for ToolSandboxPermitBinding {
    fn eq(&self, other: &Self) -> bool {
        self.context == other.context
    }
}

impl Eq for ToolSandboxPermitBinding {}

#[derive(Clone, Debug)]
pub struct ToolSandbox {
    resolver: EffectiveToolPermissionPolicyResolver,
    execution: Arc<ToolSandboxExecution>,
}

impl ToolSandbox {
    pub fn new() -> Self {
        Self::with_runtime_paths(SandboxRuntimePaths::default())
    }

    pub fn with_runtime_paths(runtime_paths: SandboxRuntimePaths) -> Self {
        Self::with_runtime_paths_and_audit_sink(
            runtime_paths,
            Arc::new(crate::audit::TracingToolSandboxAuditSink),
        )
    }

    pub fn with_runtime_paths_and_audit_sink(
        runtime_paths: SandboxRuntimePaths,
        audit_sink: Arc<dyn ToolSandboxAuditSink>,
    ) -> Self {
        let platform = host_platform();
        let assembly = build_host_providers(&runtime_paths);
        Self {
            resolver: EffectiveToolPermissionPolicyResolver,
            execution: Arc::new(ToolSandboxExecution::new(
                SandboxManager::new_with_unavailable(
                    platform,
                    assembly.providers,
                    assembly.unavailable,
                ),
                audit_sink,
            )),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_providers(
        platform: Option<SandboxProviderPlatform>,
        providers: Vec<Arc<dyn SandboxProvider>>,
    ) -> Self {
        Self::from_providers_with_audit_sink(
            platform,
            providers,
            Arc::new(crate::audit::TracingToolSandboxAuditSink),
        )
    }

    #[cfg(test)]
    pub(crate) fn from_providers_with_audit_sink(
        platform: Option<SandboxProviderPlatform>,
        providers: Vec<Arc<dyn SandboxProvider>>,
        audit_sink: Arc<dyn ToolSandboxAuditSink>,
    ) -> Self {
        Self {
            resolver: EffectiveToolPermissionPolicyResolver,
            execution: Arc::new(ToolSandboxExecution::new(
                SandboxManager::new(platform, providers),
                audit_sink,
            )),
        }
    }

    pub fn evaluate(
        &self,
        request: ToolSandboxRequest,
    ) -> Result<ToolSandboxDecision, TalosSandboxError> {
        let invocation = request.invocation.clone();
        let user_mode = request.user_mode;
        let decision = match self.resolver.resolve(request) {
            ToolSandboxDecision::Allowed(permit) => {
                let effective_policy = permit.effective_policy().clone();
                let context = ToolSandboxContext::new(
                    permit.invocation().clone(),
                    effective_policy.clone(),
                    user_mode,
                    self.execution.manager().platform(),
                )?;
                ToolSandboxDecision::Allowed(permit.bind(ToolSandboxPermitBinding {
                    context: Arc::new(context),
                    execution: self.execution.clone(),
                })?)
            }
            decision => decision,
        };
        let (outcome, effective_policy) = match &decision {
            ToolSandboxDecision::Allowed(permit) => (
                ToolSandboxAuditOutcome::Allowed,
                Some(permit.effective_policy().clone()),
            ),
            ToolSandboxDecision::RequiresApproval(_) => {
                (ToolSandboxAuditOutcome::RequiresApproval, None)
            }
            ToolSandboxDecision::NeedsRiskAssessment => {
                (ToolSandboxAuditOutcome::NeedsRiskAssessment, None)
            }
            ToolSandboxDecision::Denied(_) => (ToolSandboxAuditOutcome::Denied, None),
            ToolSandboxDecision::External(_) => (ToolSandboxAuditOutcome::External, None),
        };
        self.execution.record(&ToolSandboxAuditEvent {
            invocation,
            stage: ToolSandboxAuditStage::Evaluate,
            outcome,
            user_mode,
            effective_policy,
            enforcement: None,
            error_category: None,
        });
        Ok(decision)
    }

    /// Captures the exact bounded scope that an approval may grant for one invocation.
    pub fn approval_request(&self, request: &ToolSandboxRequest) -> crate::ToolApprovalRequest {
        crate::permission::approval_request(request)
    }

    pub fn diagnostics(&self) -> ToolSandboxDiagnostics {
        self.execution.manager().diagnostics()
    }
}

/// Supplies trusted helper paths resolved by the Talos runtime assembly layer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SandboxRuntimePaths {
    pub macos_sandbox_executable: Option<PathBuf>,
    pub linux_helper_executable: Option<PathBuf>,
}

impl Default for ToolSandbox {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSandboxDiagnostics {
    pub providers: Vec<ToolSandboxProviderDiagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSandboxProviderDiagnostic {
    pub platform: SandboxProviderPlatform,
    pub provider_name: String,
    pub status: ToolSandboxProviderStatus,
    pub capabilities: Vec<SandboxIsolationCapability>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolSandboxProviderStatus {
    Available,
    Unsupported { reason: String },
    Failed { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolSandboxEnforcement {
    None,
    InProcessAuthorizer,
    LocalOs {
        platform: SandboxProviderPlatform,
        provider_name: String,
    },
    External(ExternalSandboxEnforcement),
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum TalosSandboxError {
    #[error("sandbox is unsupported on the current platform")]
    UnsupportedPlatform,
    #[error("sandbox provider is unavailable for {platform:?}")]
    ProviderUnavailable { platform: SandboxProviderPlatform },
    #[error("sandbox capability is unsupported: {capability:?}: {reason}")]
    UnsupportedCapability {
        capability: SandboxIsolationCapability,
        reason: String,
    },
    #[error("sandbox setup failed: {reason}")]
    SandboxSetupFailed { reason: String },
    #[error("sandbox process execution failed: {reason}")]
    SandboxExecutionFailed { reason: String },
    #[error("sandbox bypass was denied")]
    SandboxBypassDenied,
    #[error("sandbox denied filesystem {operation} access to {path}")]
    FileSystemAccessDenied {
        operation: &'static str,
        path: PathBuf,
    },
    #[error("sandbox filesystem operation failed: {reason}")]
    FileSystemOperationFailed { reason: String },
    #[error("invalid local sandbox context")]
    InvalidLocalExecutionContext,
    #[error(transparent)]
    Permission(#[from] ToolPermissionError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TalosSandboxErrorCategory {
    UnsupportedPlatform,
    ProviderUnavailable,
    UnsupportedCapability,
    SetupFailed,
    ExecutionFailed,
    BypassDenied,
    FileSystemDenied,
    FileSystemFailed,
    InvalidContext,
    PermissionDenied,
}

impl TalosSandboxError {
    pub fn category(&self) -> TalosSandboxErrorCategory {
        match self {
            Self::UnsupportedPlatform => TalosSandboxErrorCategory::UnsupportedPlatform,
            Self::ProviderUnavailable { .. } => TalosSandboxErrorCategory::ProviderUnavailable,
            Self::UnsupportedCapability { .. } => TalosSandboxErrorCategory::UnsupportedCapability,
            Self::SandboxSetupFailed { .. } => TalosSandboxErrorCategory::SetupFailed,
            Self::SandboxExecutionFailed { .. } => TalosSandboxErrorCategory::ExecutionFailed,
            Self::SandboxBypassDenied => TalosSandboxErrorCategory::BypassDenied,
            Self::FileSystemAccessDenied { .. } => TalosSandboxErrorCategory::FileSystemDenied,
            Self::FileSystemOperationFailed { .. } => TalosSandboxErrorCategory::FileSystemFailed,
            Self::InvalidLocalExecutionContext => TalosSandboxErrorCategory::InvalidContext,
            Self::Permission(_) => TalosSandboxErrorCategory::PermissionDenied,
        }
    }
}

pub(crate) fn map_provider_error(error: SandboxProviderError) -> TalosSandboxError {
    match error {
        SandboxProviderError::UnsupportedCapability { capability, reason } => {
            TalosSandboxError::UnsupportedCapability { capability, reason }
        }
        SandboxProviderError::ProbeFailed { reason }
        | SandboxProviderError::InvalidIsolationSpec { reason }
        | SandboxProviderError::PrepareFailed { reason } => {
            TalosSandboxError::SandboxSetupFailed { reason }
        }
    }
}

struct HostProviderAssembly {
    providers: Vec<Arc<dyn SandboxProvider>>,
    unavailable: Option<ToolSandboxProviderDiagnostic>,
}

impl HostProviderAssembly {
    fn available(provider: Arc<dyn SandboxProvider>) -> Self {
        Self {
            providers: vec![provider],
            unavailable: None,
        }
    }

    fn unavailable(
        platform: SandboxProviderPlatform,
        provider_name: impl Into<String>,
        status: ToolSandboxProviderStatus,
    ) -> Self {
        Self {
            providers: Vec::new(),
            unavailable: Some(ToolSandboxProviderDiagnostic {
                platform,
                provider_name: provider_name.into(),
                status,
                capabilities: Vec::new(),
            }),
        }
    }
}

fn build_host_providers(runtime_paths: &SandboxRuntimePaths) -> HostProviderAssembly {
    #[cfg(target_os = "macos")]
    {
        let provider = match runtime_paths.macos_sandbox_executable.clone() {
            Some(executable) => {
                talos_macos_sandbox::MacosSandboxProvider::with_executable(executable)
            }
            None => Ok(talos_macos_sandbox::MacosSandboxProvider::new()),
        };
        match provider {
            Ok(provider) => HostProviderAssembly::available(Arc::new(provider)),
            Err(error) => HostProviderAssembly::unavailable(
                SandboxProviderPlatform::MacosSeatbelt,
                talos_macos_sandbox::MACOS_SANDBOX_PROVIDER_NAME,
                ToolSandboxProviderStatus::Failed {
                    reason: error.to_string(),
                },
            ),
        }
    }
    #[cfg(target_os = "linux")]
    {
        let Some(helper) = runtime_paths.linux_helper_executable.clone() else {
            return HostProviderAssembly::unavailable(
                SandboxProviderPlatform::LinuxBubblewrap,
                talos_linux_sandbox::LINUX_SANDBOX_PROVIDER_NAME,
                ToolSandboxProviderStatus::Unsupported {
                    reason: "Linux sandbox helper is not configured".to_string(),
                },
            );
        };
        match talos_linux_sandbox::LinuxSandboxProviderConfig::new(helper) {
            Ok(config) => HostProviderAssembly::available(Arc::new(
                talos_linux_sandbox::LinuxSandboxProvider::new(config),
            )),
            Err(error) => HostProviderAssembly::unavailable(
                SandboxProviderPlatform::LinuxBubblewrap,
                talos_linux_sandbox::LINUX_SANDBOX_PROVIDER_NAME,
                ToolSandboxProviderStatus::Failed {
                    reason: error.to_string(),
                },
            ),
        }
    }
    #[cfg(target_os = "windows")]
    {
        let _ = runtime_paths;
        return HostProviderAssembly::available(Arc::new(
            talos_windows_sandbox::WindowsSandboxProvider::new(),
        ));
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = runtime_paths;
        HostProviderAssembly {
            providers: Vec::new(),
            unavailable: None,
        }
    }
}

#[cfg(test)]
#[path = "facade_tests.rs"]
mod tests;
