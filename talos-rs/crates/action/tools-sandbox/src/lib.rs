use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolSandboxMode {
    #[default]
    UseDefault,
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolApprovalRequirement {
    Skip,
    NeedsApproval { reason: Option<String> },
    Forbidden { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolGovernanceArgumentOverride {
    #[default]
    None,
    ExecCommandPermissions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolGovernanceSpec {
    pub default_approval: ToolApprovalRequirement,
    pub preferred_sandbox: ToolSandboxMode,
    pub argument_override: ToolGovernanceArgumentOverride,
    pub retry_without_sandbox: bool,
}

impl Default for ToolGovernanceSpec {
    fn default() -> Self {
        Self {
            default_approval: ToolApprovalRequirement::Skip,
            preferred_sandbox: ToolSandboxMode::UseDefault,
            argument_override: ToolGovernanceArgumentOverride::None,
            retry_without_sandbox: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionPlan {
    pub approval: ToolApprovalRequirement,
    pub sandbox_mode: ToolSandboxMode,
    pub retry_without_sandbox: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GovernanceOutcome {
    Allowed,
    PendingApproval { prompt: String },
    Forbidden { reason: String },
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GovernanceDecisionReason {
    NoKernelConfig,
    PreApproved,
    ToolGovernanceSpec { override_applied: bool },
    GlobalPolicy { policy: ToolApprovalPolicySnapshot },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolApprovalPolicySnapshot {
    OnRequest,
    Never,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOrchestratorConfig {
    pub default_sandbox_mode: ToolSandboxMode,
}

impl Default for ToolOrchestratorConfig {
    fn default() -> Self {
        Self {
            default_sandbox_mode: ToolSandboxMode::UseDefault,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ToolPlanError {
    #[error("tool execution is forbidden: {reason}")]
    Forbidden { reason: String },
    #[error("tool approval is required{reason_suffix}")]
    ApprovalRequired { reason_suffix: String },
}

pub trait ApprovableTool<Request> {
    fn approval_requirement(&self, request: &Request) -> ToolApprovalRequirement;
}

pub trait SandboxAwareTool<Request> {
    fn sandbox_mode(&self, request: &Request, config: &ToolOrchestratorConfig) -> ToolSandboxMode;

    fn retry_without_sandbox(&self) -> bool {
        false
    }
}

#[derive(Debug, Default)]
pub struct ToolOrchestrator {
    config: ToolOrchestratorConfig,
}

impl ToolOrchestrator {
    pub fn new(config: ToolOrchestratorConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &ToolOrchestratorConfig {
        &self.config
    }

    pub fn plan<Request, Tool>(
        &self,
        tool: &Tool,
        request: &Request,
    ) -> Result<ToolExecutionPlan, ToolPlanError>
    where
        Tool: ApprovableTool<Request> + SandboxAwareTool<Request>,
    {
        let approval = tool.approval_requirement(request);
        if let ToolApprovalRequirement::Forbidden { reason } = &approval {
            return Err(ToolPlanError::Forbidden {
                reason: reason.clone(),
            });
        }

        Ok(ToolExecutionPlan {
            approval,
            sandbox_mode: tool.sandbox_mode(request, &self.config),
            retry_without_sandbox: tool.retry_without_sandbox(),
        })
    }
}

impl ToolExecutionPlan {
    pub fn ensure_approved(&self) -> Result<(), ToolPlanError> {
        match &self.approval {
            ToolApprovalRequirement::Skip => Ok(()),
            ToolApprovalRequirement::NeedsApproval { reason } => {
                Err(ToolPlanError::ApprovalRequired {
                    reason_suffix: reason
                        .as_ref()
                        .map(|value| format!(": {}", value))
                        .unwrap_or_default(),
                })
            }
            ToolApprovalRequirement::Forbidden { reason } => Err(ToolPlanError::Forbidden {
                reason: reason.clone(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct ReadOnlyTool;

    impl ApprovableTool<()> for ReadOnlyTool {
        fn approval_requirement(&self, _request: &()) -> ToolApprovalRequirement {
            ToolApprovalRequirement::Skip
        }
    }

    impl SandboxAwareTool<()> for ReadOnlyTool {
        fn sandbox_mode(&self, _request: &(), _config: &ToolOrchestratorConfig) -> ToolSandboxMode {
            ToolSandboxMode::ReadOnly
        }
    }

    #[derive(Debug)]
    struct EscalatingTool;

    impl ApprovableTool<()> for EscalatingTool {
        fn approval_requirement(&self, _request: &()) -> ToolApprovalRequirement {
            ToolApprovalRequirement::NeedsApproval {
                reason: Some("requires filesystem write".to_string()),
            }
        }
    }

    impl SandboxAwareTool<()> for EscalatingTool {
        fn sandbox_mode(&self, _request: &(), _config: &ToolOrchestratorConfig) -> ToolSandboxMode {
            ToolSandboxMode::WorkspaceWrite
        }

        fn retry_without_sandbox(&self) -> bool {
            true
        }
    }

    #[derive(Debug)]
    struct ForbiddenTool;

    impl ApprovableTool<()> for ForbiddenTool {
        fn approval_requirement(&self, _request: &()) -> ToolApprovalRequirement {
            ToolApprovalRequirement::Forbidden {
                reason: "dangerous command".to_string(),
            }
        }
    }

    impl SandboxAwareTool<()> for ForbiddenTool {
        fn sandbox_mode(&self, _request: &(), _config: &ToolOrchestratorConfig) -> ToolSandboxMode {
            ToolSandboxMode::DangerFullAccess
        }
    }

    #[test]
    fn test_orchestrator_builds_read_only_plan() {
        let orchestrator = ToolOrchestrator::new(ToolOrchestratorConfig::default());
        let plan = orchestrator
            .plan(&ReadOnlyTool, &())
            .expect("read-only tool should be plannable");

        assert_eq!(plan.approval, ToolApprovalRequirement::Skip);
        assert_eq!(plan.sandbox_mode, ToolSandboxMode::ReadOnly);
        assert!(!plan.retry_without_sandbox);
    }

    #[test]
    fn test_orchestrator_preserves_approval_and_retry_preferences() {
        let orchestrator = ToolOrchestrator::new(ToolOrchestratorConfig::default());
        let plan = orchestrator
            .plan(&EscalatingTool, &())
            .expect("escalating tool should be plannable");

        assert_eq!(
            plan.approval,
            ToolApprovalRequirement::NeedsApproval {
                reason: Some("requires filesystem write".to_string()),
            }
        );
        assert_eq!(plan.sandbox_mode, ToolSandboxMode::WorkspaceWrite);
        assert!(plan.retry_without_sandbox);
    }

    #[test]
    fn test_orchestrator_rejects_forbidden_tools() {
        let orchestrator = ToolOrchestrator::new(ToolOrchestratorConfig::default());
        let error = orchestrator
            .plan(&ForbiddenTool, &())
            .expect_err("forbidden tool should be rejected");

        assert_eq!(
            error,
            ToolPlanError::Forbidden {
                reason: "dangerous command".to_string(),
            }
        );
    }

    #[test]
    fn test_execution_plan_requires_approval_before_execution() {
        let orchestrator = ToolOrchestrator::new(ToolOrchestratorConfig::default());
        let plan = orchestrator
            .plan(&EscalatingTool, &())
            .expect("escalating tool should be plannable");

        let error = plan
            .ensure_approved()
            .expect_err("approval should be required");

        assert_eq!(
            error,
            ToolPlanError::ApprovalRequired {
                reason_suffix: ": requires filesystem write".to_string(),
            }
        );
    }
}
