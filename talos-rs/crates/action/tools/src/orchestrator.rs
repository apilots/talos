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
#[path = "orchestrator_tests.rs"]
mod tests;
