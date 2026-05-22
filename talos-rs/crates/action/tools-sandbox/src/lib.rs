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
    pub retry_without_sandbox: bool,
}

impl Default for ToolGovernanceSpec {
    fn default() -> Self {
        Self {
            preferred_sandbox: ToolSandboxMode::UseDefault,
            argument_override: ToolGovernanceArgumentOverride::None,
            retry_without_sandbox: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionPlan {
    pub sandbox_mode: ToolSandboxMode,
    pub retry_without_sandbox: bool,
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
        Tool: SandboxAwareTool<Request>,
    {
        Ok(ToolExecutionPlan {
            sandbox_mode: tool.sandbox_mode(request, &self.config),
            retry_without_sandbox: tool.retry_without_sandbox(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct ReadOnlyTool;

    impl SandboxAwareTool<()> for ReadOnlyTool {
        fn sandbox_mode(&self, _request: &(), _config: &ToolOrchestratorConfig) -> ToolSandboxMode {
            ToolSandboxMode::ReadOnly
        }
    }

    #[derive(Debug)]
    struct EscalatingTool;

    impl SandboxAwareTool<()> for EscalatingTool {
        fn sandbox_mode(&self, _request: &(), _config: &ToolOrchestratorConfig) -> ToolSandboxMode {
            ToolSandboxMode::WorkspaceWrite
        }

        fn retry_without_sandbox(&self) -> bool {
            true
        }
    }

    #[test]
    fn test_orchestrator_builds_read_only_plan() {
        let orchestrator = ToolOrchestrator::new(ToolOrchestratorConfig::default());
        let plan = orchestrator
            .plan(&ReadOnlyTool, &())
            .expect("read-only tool should be plannable");

        assert_eq!(plan.sandbox_mode, ToolSandboxMode::ReadOnly);
        assert!(!plan.retry_without_sandbox);
    }

    #[test]
    fn test_orchestrator_preserves_sandbox_and_retry_preferences() {
        let orchestrator = ToolOrchestrator::new(ToolOrchestratorConfig::default());
        let plan = orchestrator
            .plan(&EscalatingTool, &())
            .expect("escalating tool should be plannable");

        assert_eq!(plan.sandbox_mode, ToolSandboxMode::WorkspaceWrite);
        assert!(plan.retry_without_sandbox);
    }
}
