use crate::{
    EffectiveToolPermissionPolicy, TalosSandboxErrorCategory, ToolInvocationIdentity,
    ToolSandboxEnforcement,
};
use talos_types::UserPermissionMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolSandboxAuditStage {
    Evaluate,
    SpawnProcess,
    FileSystem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolSandboxAuditOutcome {
    Allowed,
    RequiresApproval,
    NeedsRiskAssessment,
    Denied,
    External,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSandboxAuditEvent {
    pub invocation: ToolInvocationIdentity,
    pub stage: ToolSandboxAuditStage,
    pub outcome: ToolSandboxAuditOutcome,
    pub user_mode: UserPermissionMode,
    pub effective_policy: Option<EffectiveToolPermissionPolicy>,
    pub enforcement: Option<ToolSandboxEnforcement>,
    pub error_category: Option<TalosSandboxErrorCategory>,
}

/// Receives decision and execution facts from the sandbox facade.
///
/// Implementations must not make permission decisions or select providers.
pub trait ToolSandboxAuditSink: Send + Sync {
    fn record(&self, event: &ToolSandboxAuditEvent);
}

#[derive(Debug, Default)]
pub(crate) struct TracingToolSandboxAuditSink;

impl ToolSandboxAuditSink for TracingToolSandboxAuditSink {
    fn record(&self, event: &ToolSandboxAuditEvent) {
        tracing::info!(target: "talos_sandbox_audit", ?event, "tool sandbox audit event");
    }
}
