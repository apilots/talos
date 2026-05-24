use std::sync::Arc;

use serde_json::Value;
use talos_config::ToolSandboxModeConfig;
use talos_error::{TalosError, TalosResult};
use talos_tools_sandbox::{
    GovernanceDecisionReason, GovernanceEventSink, GovernanceOutcome, ToolExecutionPlan,
    ToolGovernanceArgumentOverride, ToolGovernanceDecision, ToolGovernanceSpec, ToolSandboxMode,
};

use crate::context::ToolContext;
use crate::invocation::{ToolCallPayload, ToolDispatchContext, ToolInvocation};
use crate::output::ToolDispatchOutcome;
use crate::spec::RegisteredToolSpec;

pub struct RegistryOrchestrator {
    default_sandbox_mode: ToolSandboxMode,
    sink: Option<Arc<dyn GovernanceEventSink>>,
}

impl RegistryOrchestrator {
    pub fn from_context(context: &ToolContext) -> Self {
        let default_sandbox_mode = context
            .kernel_config()
            .map(|config| map_sandbox_mode(&config.tools.sandbox_policy.default_mode))
            .unwrap_or(ToolSandboxMode::UseDefault);
        Self {
            default_sandbox_mode,
            sink: None,
        }
    }

    pub fn with_sink(mut self, sink: Arc<dyn GovernanceEventSink>) -> Self {
        self.sink = Some(sink);
        self
    }

    pub fn plan(
        &self,
        invocation: &ToolInvocation,
        context: &ToolDispatchContext,
        spec: Option<&RegisteredToolSpec>,
    ) -> TalosResult<Option<ToolExecutionPlan>> {
        let Some(_config) = context.tool_context().kernel_config() else {
            self.emit_decision(
                invocation,
                GovernanceOutcome::Skipped,
                ToolSandboxMode::UseDefault,
                GovernanceDecisionReason::NoKernelConfig,
            );
            return Ok(None);
        };

        let Some(spec) = spec else {
            return Ok(None);
        };

        let (governance, override_applied, policy_reason) =
            self.resolve_governance(spec, invocation)?;
        let sandbox_mode = match governance.preferred_sandbox {
            ToolSandboxMode::UseDefault => self.default_sandbox_mode,
            other => other,
        };
        let plan = ToolExecutionPlan {
            sandbox_mode,
            retry_without_sandbox: governance.retry_without_sandbox,
        };

        self.emit_decision(
            invocation,
            GovernanceOutcome::Allowed,
            plan.sandbox_mode,
            policy_reason
                .unwrap_or(GovernanceDecisionReason::ToolGovernanceSpec { override_applied }),
        );

        Ok(Some(plan))
    }

    pub fn resolve_pending(
        &self,
        invocation: &ToolInvocation,
        context: &ToolDispatchContext,
        spec: Option<&RegisteredToolSpec>,
    ) -> TalosResult<Option<ToolDispatchOutcome>> {
        let Some(plan) = self.plan(invocation, context, spec)? else {
            return Ok(None);
        };
        let _ = plan;
        Ok(None)
    }

    fn resolve_governance(
        &self,
        spec: &RegisteredToolSpec,
        invocation: &ToolInvocation,
    ) -> TalosResult<(ToolGovernanceSpec, bool, Option<GovernanceDecisionReason>)> {
        let mut governance = spec.governance.clone();
        let mut override_applied = false;
        let policy_reason = None;

        match governance.argument_override {
            ToolGovernanceArgumentOverride::None => {}
            ToolGovernanceArgumentOverride::BashCommandPermissions => {
                let ToolCallPayload::Function { arguments } = &invocation.payload else {
                    return Err(TalosError::ToolError(
                        "shell tool requires function payload".to_string(),
                    ));
                };
                let requested_escalation = arguments
                    .get("dangerously_disable_sandbox")
                    .and_then(Value::as_bool)
                    == Some(true)
                    || arguments.get("sandbox_permissions").and_then(Value::as_str)
                        == Some("require_escalated");
                if requested_escalation {
                    override_applied = true;
                    governance.preferred_sandbox = ToolSandboxMode::DangerFullAccess;
                    governance.retry_without_sandbox = true;
                }
            }
        }

        Ok((governance, override_applied, policy_reason))
    }

    fn emit_decision(
        &self,
        invocation: &ToolInvocation,
        outcome: GovernanceOutcome,
        sandbox_mode: ToolSandboxMode,
        decision_reason: GovernanceDecisionReason,
    ) {
        if let Some(sink) = &self.sink {
            sink.on_decision(&ToolGovernanceDecision {
                tool_name: invocation.tool_name.clone(),
                call_id: invocation.call_id.clone(),
                outcome,
                sandbox_mode,
                decision_reason,
            });
        }
    }
}

fn map_sandbox_mode(mode: &ToolSandboxModeConfig) -> ToolSandboxMode {
    match mode {
        ToolSandboxModeConfig::UseDefault => ToolSandboxMode::UseDefault,
        ToolSandboxModeConfig::ReadOnly => ToolSandboxMode::ReadOnly,
        ToolSandboxModeConfig::WorkspaceWrite => ToolSandboxMode::WorkspaceWrite,
        ToolSandboxModeConfig::DangerFullAccess => ToolSandboxMode::DangerFullAccess,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invocation::ToolInvocation;
    use crate::spec::{RegisteredToolSpec, ToolParallelism, ToolPayloadKind, ToolSource};
    use serde_json::json;
    use talos_config::AgentKernelConfig;
    use talos_llm_provider::ToolDefinition;
    use talos_tools_sandbox::ToolGovernanceSpec;

    fn test_context() -> ToolDispatchContext {
        let config = AgentKernelConfig::default();
        ToolDispatchContext::new(ToolContext::new_with_kernel_config(
            "session-1",
            "user-1",
            Some(Arc::new(config)),
        ))
    }

    fn exec_spec() -> RegisteredToolSpec {
        RegisteredToolSpec::new(
            ToolDefinition::new(
                "Bash",
                "run shell command",
                Some(json!({ "type": "object" })),
            ),
            ToolSource::Local,
            ToolPayloadKind::Function,
            ToolParallelism::SerialOnly,
            true,
            ToolGovernanceSpec {
                preferred_sandbox: ToolSandboxMode::UseDefault,
                argument_override: ToolGovernanceArgumentOverride::BashCommandPermissions,
                retry_without_sandbox: true,
            },
        )
    }

    #[test]
    fn test_plan_uses_default_governance_when_no_override() {
        let orchestrator = RegistryOrchestrator::from_context(&ToolContext::default());
        let spec = RegisteredToolSpec::new(
            ToolDefinition::new("Write", "write file", Some(json!({ "type": "object" }))),
            ToolSource::Local,
            ToolPayloadKind::Function,
            ToolParallelism::SerialOnly,
            true,
            ToolGovernanceSpec::default(),
        );
        let plan = orchestrator
            .plan(
                &ToolInvocation::function("call-1", "Write", json!({})),
                &test_context(),
                Some(&spec),
            )
            .expect("plan should succeed")
            .expect("plan should exist");
        assert_eq!(plan.sandbox_mode, ToolSandboxMode::UseDefault);
    }

    #[test]
    fn test_bash_command_override_uses_danger_full_access() {
        let orchestrator = RegistryOrchestrator::from_context(&ToolContext::default());
        let plan = orchestrator
            .plan(
                &ToolInvocation::function(
                    "call-1",
                    "Bash",
                    json!({
                        "command": "git pull",
                        "dangerously_disable_sandbox": true,
                        "description": "need to fetch remote changes"
                    }),
                ),
                &test_context(),
                Some(&exec_spec()),
            )
            .expect("plan should succeed")
            .expect("plan should exist");
        assert_eq!(plan.sandbox_mode, ToolSandboxMode::DangerFullAccess);
    }

    #[test]
    fn test_mutating_tools_execute_under_configured_policy() {
        let context = test_context();
        let orchestrator = RegistryOrchestrator::from_context(context.tool_context());
        let spec = RegisteredToolSpec::new(
            ToolDefinition::new("Write", "write file", Some(json!({ "type": "object" }))),
            ToolSource::Local,
            ToolPayloadKind::Function,
            ToolParallelism::SerialOnly,
            true,
            ToolGovernanceSpec::default(),
        );

        let plan = orchestrator
            .plan(
                &ToolInvocation::function("call-2", "Write", json!({ "file": "x" })),
                &context,
                Some(&spec),
            )
            .expect("plan should succeed")
            .expect("plan should exist");
        assert_eq!(plan.sandbox_mode, ToolSandboxMode::UseDefault);
    }

    #[test]
    fn test_collaboration_tools_execute_under_configured_policy() {
        let context = test_context();
        let orchestrator = RegistryOrchestrator::from_context(context.tool_context());
        let spec = RegisteredToolSpec::new(
            ToolDefinition::new(
                "Agent",
                "spawn sub-agent",
                Some(json!({ "type": "object" })),
            ),
            ToolSource::Collaboration,
            ToolPayloadKind::Function,
            ToolParallelism::SerialOnly,
            true,
            ToolGovernanceSpec::default(),
        );

        let plan = orchestrator
            .plan(
                &ToolInvocation::function("call-3", "Agent", json!({ "description": "count" })),
                &context,
                Some(&spec),
            )
            .expect("plan should succeed")
            .expect("plan should exist");

        assert_eq!(plan.sandbox_mode, ToolSandboxMode::UseDefault);
    }
}
