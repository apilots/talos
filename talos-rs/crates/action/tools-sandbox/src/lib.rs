use serde::{Deserialize, Serialize};

mod audit;
mod facade;
mod file_system;
mod manager;
mod permission;
mod process;
mod risk;

pub use facade::{
    SandboxRuntimePaths, TalosSandboxError, TalosSandboxErrorCategory, ToolSandbox,
    ToolSandboxDiagnostics, ToolSandboxEnforcement, ToolSandboxProviderDiagnostic,
    ToolSandboxProviderStatus,
};
pub use file_system::{ToolDirectoryEntry, ToolFileMetadata, ToolFileSystemHandle};
pub use process::{
    RunningToolProcess, ToolExecutionFuture, ToolProcessCommand, ToolProcessTerminationTarget,
};
pub use risk::{CommandRisk, CommandRiskAssessment, CommandRiskCategory, CommandRiskVerdict};

pub use permission::{
    ApprovalPolicy, EffectiveToolPermissionPolicy, ExternalSandboxEnforcement,
    ExternalSandboxEnforcementSource, FileSystemOperation, FileSystemPermissionLevel,
    FileSystemSandboxPolicy, FileSystemScope, NetworkSandboxPolicy, PolicyPrecheck,
    RequestedFileSystemAccess, RequestedToolPermissions, SandboxCapability, ToolApprovalGrant,
    ToolApprovalRequest, ToolEscalationPolicy, ToolExecutionLocation, ToolGovernanceConstraints,
    ToolInvocationIdentity, ToolPermissionError, ToolPermissionProfile, ToolSandboxDecision,
    ToolSandboxDenial, ToolSandboxEnvironment, ToolSandboxOperationKind, ToolSandboxPermit,
    ToolSandboxRequest, WorkspaceWritePolicy,
};

pub use talos_sandbox_spi::{
    AbsolutePathBuf, SandboxIsolationCapability, SandboxProviderPlatform, SandboxStdin,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolSandboxMode {
    #[default]
    UseDefault,
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolGovernanceArgumentOverride {
    #[default]
    None,
    BashCommandPermissions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolGovernanceSpec {
    pub preferred_sandbox: ToolSandboxMode,
    pub argument_override: ToolGovernanceArgumentOverride,
    pub escalation: ToolEscalationPolicy,
}

impl Default for ToolGovernanceSpec {
    fn default() -> Self {
        Self {
            preferred_sandbox: ToolSandboxMode::UseDefault,
            argument_override: ToolGovernanceArgumentOverride::None,
            escalation: ToolEscalationPolicy::Forbidden,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionPlan {
    pub sandbox_mode: ToolSandboxMode,
    pub escalation: ToolEscalationPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GovernanceOutcome {
    Allowed,
    Forbidden { reason: String },
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GovernanceDecisionReason {
    NoKernelConfig,
    PreApproved,
    ToolGovernanceSpec { override_applied: bool },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolGovernanceDecision {
    pub tool_name: String,
    pub call_id: String,
    pub outcome: GovernanceOutcome,
    pub sandbox_mode: ToolSandboxMode,
    pub decision_reason: GovernanceDecisionReason,
}

pub trait GovernanceEventSink: Send + Sync {
    fn on_decision(&self, decision: &ToolGovernanceDecision);
}

pub use audit::{
    ToolSandboxAuditEvent, ToolSandboxAuditOutcome, ToolSandboxAuditSink, ToolSandboxAuditStage,
};
