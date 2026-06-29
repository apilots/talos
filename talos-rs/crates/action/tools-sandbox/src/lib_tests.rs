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
