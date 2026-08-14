use std::collections::BTreeSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use talos_types::UserPermissionMode;
use thiserror::Error;

use crate::facade::ToolSandboxPermitBinding;
use crate::AbsolutePathBuf;

mod resolver;

pub use resolver::EffectiveToolPermissionPolicyResolver;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalPolicy {
    pub require_for_network: bool,
    pub require_for_workspace_external_write: bool,
}

impl ApprovalPolicy {
    pub fn none() -> Self {
        Self {
            require_for_network: false,
            require_for_workspace_external_write: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FileSystemPermissionLevel {
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum NetworkSandboxPolicy {
    Restricted,
    Enabled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileSystemScope {
    pub readable_roots: Vec<AbsolutePathBuf>,
    pub denied_roots: Vec<AbsolutePathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceWritePolicy {
    pub readable_roots: Vec<AbsolutePathBuf>,
    pub writable_roots: Vec<AbsolutePathBuf>,
    pub denied_roots: Vec<AbsolutePathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileSystemSandboxPolicy {
    ReadOnly(FileSystemScope),
    WorkspaceWrite(WorkspaceWritePolicy),
    DangerFullAccess,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolPermissionProfile {
    pub file_system: FileSystemSandboxPolicy,
    pub network: NetworkSandboxPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxCapability {
    Enabled,
    Disabled,
    External,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveToolPermissionPolicy {
    pub approval: ApprovalPolicy,
    pub permissions: ToolPermissionProfile,
    pub sandbox: SandboxCapability,
}

impl EffectiveToolPermissionPolicy {
    pub fn new(
        approval: ApprovalPolicy,
        permissions: ToolPermissionProfile,
        sandbox: SandboxCapability,
    ) -> Result<Self, ToolPermissionError> {
        if sandbox == SandboxCapability::Disabled
            && (permissions.file_system != FileSystemSandboxPolicy::DangerFullAccess
                || permissions.network != NetworkSandboxPolicy::Enabled)
        {
            return Err(ToolPermissionError::InvalidEffectivePolicy);
        }
        Ok(Self {
            approval,
            permissions,
            sandbox,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ToolInvocationIdentity {
    pub call_id: String,
    pub tool_name: String,
    scope_digest: Option<String>,
}

impl ToolInvocationIdentity {
    pub fn new(call_id: impl Into<String>, tool_name: impl Into<String>) -> Self {
        Self {
            call_id: call_id.into(),
            tool_name: tool_name.into(),
            scope_digest: None,
        }
    }

    pub fn with_scope_digest(mut self, scope_digest: impl Into<String>) -> Self {
        self.scope_digest = Some(scope_digest.into());
        self
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ToolSandboxOperationKind {
    SpawnProcess,
    FileSystem,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FileSystemOperation {
    Read,
    Write,
}

const MAX_REQUESTED_FILE_SYSTEM_PATHS: usize = 128;

/// Bounded, normalized filesystem facts requested by one tool invocation.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RequestedFileSystemAccess {
    pub operation: FileSystemOperation,
    pub paths: Vec<AbsolutePathBuf>,
}

impl RequestedFileSystemAccess {
    pub fn new(
        operation: FileSystemOperation,
        paths: Vec<AbsolutePathBuf>,
    ) -> Result<Self, ToolPermissionError> {
        let paths = compact_paths(paths);
        if paths.is_empty() {
            return Err(ToolPermissionError::MissingFileSystemPath);
        }
        if paths.len() > MAX_REQUESTED_FILE_SYSTEM_PATHS {
            return Err(ToolPermissionError::TooManyFileSystemPaths {
                maximum: MAX_REQUESTED_FILE_SYSTEM_PATHS,
            });
        }
        Ok(Self { operation, paths })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestedToolPermissions {
    pub file_system: FileSystemPermissionLevel,
    pub network: NetworkSandboxPolicy,
    pub command_risk: Option<crate::CommandRisk>,
    pub operations: BTreeSet<ToolSandboxOperationKind>,
    pub file_system_access: Vec<RequestedFileSystemAccess>,
}

impl RequestedToolPermissions {
    pub fn new(
        file_system: FileSystemPermissionLevel,
        network: NetworkSandboxPolicy,
        command_risk: Option<crate::CommandRisk>,
        operations: BTreeSet<ToolSandboxOperationKind>,
    ) -> Result<Self, ToolPermissionError> {
        if operations.is_empty() {
            return Err(ToolPermissionError::MissingOperation);
        }
        Ok(Self {
            file_system,
            network,
            command_risk,
            operations,
            file_system_access: Vec::new(),
        })
    }

    pub fn with_file_system_access(
        mut self,
        mut file_system_access: Vec<RequestedFileSystemAccess>,
    ) -> Result<Self, ToolPermissionError> {
        file_system_access.sort();
        file_system_access.dedup();
        let total_paths = file_system_access
            .iter()
            .map(|access| access.paths.len())
            .sum::<usize>();
        if total_paths > MAX_REQUESTED_FILE_SYSTEM_PATHS {
            return Err(ToolPermissionError::TooManyFileSystemPaths {
                maximum: MAX_REQUESTED_FILE_SYSTEM_PATHS,
            });
        }
        self.file_system_access = file_system_access;
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolEscalationPolicy {
    Forbidden,
    #[default]
    RequiresApproval,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ToolGovernanceConstraints {
    pub maximum_file_system: Option<FileSystemPermissionLevel>,
    pub maximum_network: Option<NetworkSandboxPolicy>,
    pub escalation: ToolEscalationPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolApprovalGrant {
    invocation: ToolInvocationIdentity,
    file_system: FileSystemPermissionLevel,
    network: NetworkSandboxPolicy,
    operations: BTreeSet<ToolSandboxOperationKind>,
    file_system_access: Vec<RequestedFileSystemAccess>,
}

impl ToolApprovalGrant {
    fn new(
        invocation: ToolInvocationIdentity,
        file_system: FileSystemPermissionLevel,
        network: NetworkSandboxPolicy,
        operations: BTreeSet<ToolSandboxOperationKind>,
        file_system_access: Vec<RequestedFileSystemAccess>,
    ) -> Self {
        Self {
            invocation,
            file_system,
            network,
            operations,
            file_system_access,
        }
    }

    fn covers(
        &self,
        invocation: &ToolInvocationIdentity,
        requested: &RequestedToolPermissions,
    ) -> bool {
        self.invocation == *invocation
            && self.file_system >= requested.file_system
            && self.network >= requested.network
            && self.operations.is_superset(&requested.operations)
            && self.file_system_access == requested.file_system_access
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolApprovalRequest {
    pub invocation: ToolInvocationIdentity,
    pub requested_permissions: RequestedToolPermissions,
    fingerprint: String,
}

impl ToolApprovalRequest {
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn allow_once(
        &self,
        expected_fingerprint: &str,
    ) -> Result<ToolApprovalGrant, ToolPermissionError> {
        if self.fingerprint != expected_fingerprint {
            return Err(ToolPermissionError::ApprovalScopeMismatch);
        }
        Ok(ToolApprovalGrant::new(
            self.invocation.clone(),
            self.requested_permissions.file_system,
            self.requested_permissions.network,
            self.requested_permissions.operations.clone(),
            self.requested_permissions.file_system_access.clone(),
        ))
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalSandboxEnforcementSource {
    RemoteEnvironment,
    ServerDeclared,
    None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalSandboxEnforcement {
    pub source: ExternalSandboxEnforcementSource,
}

impl ExternalSandboxEnforcement {
    pub fn new(source: ExternalSandboxEnforcementSource) -> Self {
        Self { source }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolExecutionLocation {
    Local,
    External(ExternalSandboxEnforcement),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSandboxEnvironment {
    pub workspace_roots: Vec<AbsolutePathBuf>,
    pub denied_roots: Vec<AbsolutePathBuf>,
    pub location: ToolExecutionLocation,
}

impl ToolSandboxEnvironment {
    pub fn local(
        workspace_roots: Vec<AbsolutePathBuf>,
        denied_roots: Vec<AbsolutePathBuf>,
    ) -> Result<Self, ToolPermissionError> {
        if workspace_roots.is_empty() {
            return Err(ToolPermissionError::MissingWorkspaceRoot);
        }
        Ok(Self {
            workspace_roots: compact_paths(workspace_roots),
            denied_roots: compact_paths(denied_roots),
            location: ToolExecutionLocation::Local,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSandboxRequest {
    pub invocation: ToolInvocationIdentity,
    pub user_mode: UserPermissionMode,
    pub governance: ToolGovernanceConstraints,
    pub requested_permissions: RequestedToolPermissions,
    pub command_risk_assessment: Option<crate::CommandRiskAssessment>,
    pub approval_grant: Option<ToolApprovalGrant>,
    pub environment: ToolSandboxEnvironment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolSandboxDenial {
    GovernanceConstraint { reason: String },
    EscalationForbidden,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyPrecheck {
    Allowed(EffectiveToolPermissionPolicy),
    RequiresApproval(ToolApprovalRequest),
    NeedsRiskAssessment,
    Denied(ToolSandboxDenial),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolSandboxDecision {
    Allowed(ToolSandboxPermit),
    RequiresApproval(ToolApprovalRequest),
    NeedsRiskAssessment,
    Denied(ToolSandboxDenial),
    External(ExternalSandboxEnforcement),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSandboxPermit {
    invocation: ToolInvocationIdentity,
    allowed_operations: BTreeSet<ToolSandboxOperationKind>,
    effective_policy: EffectiveToolPermissionPolicy,
    binding: Option<Arc<ToolSandboxPermitBinding>>,
}

impl ToolSandboxPermit {
    pub(crate) fn new(
        invocation: ToolInvocationIdentity,
        allowed_operations: BTreeSet<ToolSandboxOperationKind>,
        effective_policy: EffectiveToolPermissionPolicy,
    ) -> Self {
        let permit = Self {
            invocation,
            allowed_operations,
            effective_policy,
            binding: None,
        };
        debug_assert!(permit.allowed_operations.iter().all(|operation| permit
            .require_operation(&permit.invocation, *operation)
            .is_ok()));
        permit
    }

    pub fn effective_policy(&self) -> &EffectiveToolPermissionPolicy {
        &self.effective_policy
    }

    pub fn invocation(&self) -> &ToolInvocationIdentity {
        &self.invocation
    }

    pub(crate) fn bind(
        mut self,
        binding: ToolSandboxPermitBinding,
    ) -> Result<Self, ToolPermissionError> {
        if self.binding.is_some() || binding.context.effective_policy != self.effective_policy {
            return Err(ToolPermissionError::InvalidPermitBinding);
        }
        self.binding = Some(Arc::new(binding));
        Ok(self)
    }

    pub(crate) fn binding(&self) -> Result<&ToolSandboxPermitBinding, ToolPermissionError> {
        self.binding
            .as_deref()
            .ok_or(ToolPermissionError::InvalidPermitBinding)
    }

    pub(crate) fn require_operation(
        &self,
        invocation: &ToolInvocationIdentity,
        operation: ToolSandboxOperationKind,
    ) -> Result<(), ToolPermissionError> {
        if self.invocation != *invocation {
            return Err(ToolPermissionError::InvocationMismatch);
        }
        if !self.allowed_operations.contains(&operation) {
            return Err(ToolPermissionError::OperationNotGranted { operation });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ToolPermissionError {
    #[error("sandbox request must include at least one operation")]
    MissingOperation,
    #[error("filesystem access must include at least one path")]
    MissingFileSystemPath,
    #[error("filesystem access exceeds the path limit of {maximum}")]
    TooManyFileSystemPaths { maximum: usize },
    #[error("local sandbox environment requires at least one workspace root")]
    MissingWorkspaceRoot,
    #[error("invalid effective tool permission policy")]
    InvalidEffectivePolicy,
    #[error("sandbox permit belongs to another invocation")]
    InvocationMismatch,
    #[error("sandbox operation was not granted: {operation:?}")]
    OperationNotGranted { operation: ToolSandboxOperationKind },
    #[error("sandbox permit is not bound to its execution context")]
    InvalidPermitBinding,
    #[error("sandbox approval scope does not match the original request")]
    ApprovalScopeMismatch,
}

pub(crate) fn approval_request(request: &ToolSandboxRequest) -> ToolApprovalRequest {
    ToolApprovalRequest {
        invocation: request.invocation.clone(),
        requested_permissions: request.requested_permissions.clone(),
        fingerprint: request_fingerprint(request),
    }
}

fn request_fingerprint(request: &ToolSandboxRequest) -> String {
    let mut scope = String::new();
    use std::fmt::Write;
    let _ = write!(
        scope,
        "call={}|tool={}|scope={:?}|mode={:?}|fs={:?}|network={:?}|risk={:?}|escalation={:?}|max_fs={:?}|max_network={:?}",
        request.invocation.call_id,
        request.invocation.tool_name,
        request.invocation.scope_digest,
        request.user_mode,
        request.requested_permissions.file_system,
        request.requested_permissions.network,
        request.requested_permissions.command_risk,
        request.governance.escalation,
        request.governance.maximum_file_system,
        request.governance.maximum_network,
    );
    for operation in &request.requested_permissions.operations {
        let _ = write!(scope, "|operation={operation:?}");
    }
    for access in &request.requested_permissions.file_system_access {
        let _ = write!(scope, "|fs_operation={:?}", access.operation);
        for path in &access.paths {
            let _ = write!(scope, "|fs_path={}", path.as_path().display());
        }
    }
    for root in &request.environment.workspace_roots {
        let _ = write!(scope, "|workspace={}", root.as_path().display());
    }
    for root in &request.environment.denied_roots {
        let _ = write!(scope, "|denied={}", root.as_path().display());
    }
    let _ = write!(scope, "|location={:?}", request.environment.location);
    format!("sar_{:x}", Sha256::digest(scope.as_bytes()))
}

fn compact_paths(paths: Vec<AbsolutePathBuf>) -> Vec<AbsolutePathBuf> {
    paths
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
#[path = "permission_tests.rs"]
mod tests;
