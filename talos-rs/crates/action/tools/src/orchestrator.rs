use std::sync::Arc;

use serde_json::Value;
use talos_config::{ToolApprovalPolicy, ToolSandboxModeConfig};
use talos_error::{TalosError, TalosResult};
use talos_tools_sandbox::{
    GovernanceDecisionReason, GovernanceEventSink, GovernanceOutcome, ToolApprovalPolicySnapshot,
    ToolApprovalRequirement, ToolExecutionPlan, ToolGovernanceArgumentOverride,
    ToolGovernanceDecision, ToolGovernanceSpec, ToolPlanError, ToolSandboxMode,
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
        if context.tool_context().pre_approved() {
            self.emit_decision(
                invocation,
                GovernanceOutcome::Skipped,
                ToolSandboxMode::UseDefault,
                GovernanceDecisionReason::PreApproved,
            );
            return Ok(None);
        }

        let Some(config) = context.tool_context().kernel_config() else {
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
            self.resolve_governance(spec, invocation, config.tools.approval_policy.clone())?;
        let sandbox_mode = match governance.preferred_sandbox {
            ToolSandboxMode::UseDefault => self.default_sandbox_mode,
            other => other,
        };
        let plan = ToolExecutionPlan {
            approval: governance.default_approval.clone(),
            sandbox_mode,
            retry_without_sandbox: governance.retry_without_sandbox,
        };

        let outcome = match &plan.approval {
            ToolApprovalRequirement::Skip => GovernanceOutcome::Allowed,
            ToolApprovalRequirement::NeedsApproval { reason } => {
                GovernanceOutcome::PendingApproval {
                    prompt: approval_prompt_from_reason(reason.as_ref()),
                }
            }
            ToolApprovalRequirement::Forbidden { reason } => GovernanceOutcome::Forbidden {
                reason: reason.clone(),
            },
        };
        self.emit_decision(
            invocation,
            outcome,
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
        match plan.ensure_approved() {
            Ok(()) => Ok(None),
            Err(ToolPlanError::ApprovalRequired { .. }) => {
                let prompt = approval_prompt(&plan.approval);
                let arguments = invocation_arguments(invocation);
                Ok(Some(ToolDispatchOutcome::pending_approval(
                    invocation.tool_name.clone(),
                    prompt,
                    arguments,
                )))
            }
            Err(ToolPlanError::Forbidden { reason }) => Err(TalosError::ToolError(reason)),
        }
    }

    fn resolve_governance(
        &self,
        spec: &RegisteredToolSpec,
        invocation: &ToolInvocation,
        approval_policy: ToolApprovalPolicy,
    ) -> TalosResult<(ToolGovernanceSpec, bool, Option<GovernanceDecisionReason>)> {
        let mut governance = spec.governance.clone();
        let mut override_applied = false;
        let mut policy_reason = None;

        match governance.argument_override {
            ToolGovernanceArgumentOverride::None => {}
            ToolGovernanceArgumentOverride::ExecCommandPermissions => {
                let ToolCallPayload::Function { arguments } = &invocation.payload else {
                    return Err(TalosError::ToolError(
                        "exec_command requires function payload".to_string(),
                    ));
                };
                if arguments.get("sandbox_permissions").and_then(Value::as_str)
                    == Some("require_escalated")
                {
                    override_applied = true;
                    governance.preferred_sandbox = ToolSandboxMode::DangerFullAccess;
                    governance.retry_without_sandbox = true;
                    let reason = arguments
                        .get("justification")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| {
                            Some("exec_command requested escalated permissions".to_string())
                        });
                    match approval_policy {
                        ToolApprovalPolicy::Never => {
                            governance.default_approval = ToolApprovalRequirement::Forbidden {
                                reason: reason.clone().unwrap_or_else(|| {
                                    "exec_command escalation is disabled".to_string()
                                }),
                            };
                            policy_reason = Some(GovernanceDecisionReason::GlobalPolicy {
                                policy: ToolApprovalPolicySnapshot::Never,
                            });
                        }
                        ToolApprovalPolicy::OnRequest => {
                            governance.default_approval =
                                ToolApprovalRequirement::NeedsApproval { reason };
                            policy_reason = Some(GovernanceDecisionReason::GlobalPolicy {
                                policy: ToolApprovalPolicySnapshot::OnRequest,
                            });
                        }
                    }
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

fn approval_prompt_from_reason(reason: Option<&String>) -> String {
    reason
        .map(|value| format!("tool approval is required: {value}"))
        .unwrap_or_else(|| "tool approval is required".to_string())
}

fn approval_prompt(requirement: &ToolApprovalRequirement) -> String {
    match requirement {
        ToolApprovalRequirement::NeedsApproval { reason } => {
            approval_prompt_from_reason(reason.as_ref())
        }
        ToolApprovalRequirement::Skip => "tool approval is required".to_string(),
        ToolApprovalRequirement::Forbidden { reason } => reason.clone(),
    }
}

fn invocation_arguments(invocation: &ToolInvocation) -> Value {
    match &invocation.payload {
        ToolCallPayload::Function { arguments } => arguments.clone(),
        ToolCallPayload::Mcp { arguments, .. } => arguments.clone(),
        ToolCallPayload::Custom { input } => serde_json::json!({ "input": input }),
        ToolCallPayload::Resource { uri } => serde_json::json!({ "uri": uri }),
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
    use talos_tools_sandbox::{ToolApprovalRequirement, ToolGovernanceSpec};

    fn test_context() -> ToolDispatchContext {
        let mut config = AgentKernelConfig::default();
        config.tools.approval_policy = ToolApprovalPolicy::OnRequest;
        ToolDispatchContext::new(ToolContext::new_with_kernel_config(
            "session-1",
            "user-1",
            Some(Arc::new(config)),
        ))
    }

    fn exec_spec() -> RegisteredToolSpec {
        RegisteredToolSpec::new(
            ToolDefinition::new(
                "exec_command",
                "run command",
                Some(json!({ "type": "object" })),
            ),
            ToolSource::Local,
            ToolPayloadKind::Function,
            ToolParallelism::SerialOnly,
            true,
            ToolGovernanceSpec {
                default_approval: ToolApprovalRequirement::Skip,
                preferred_sandbox: ToolSandboxMode::UseDefault,
                argument_override: ToolGovernanceArgumentOverride::ExecCommandPermissions,
                retry_without_sandbox: true,
            },
        )
    }

    #[test]
    fn test_plan_uses_default_governance_when_no_override() {
        let orchestrator = RegistryOrchestrator::from_context(&ToolContext::default());
        let spec = RegisteredToolSpec::new(
            ToolDefinition::new(
                "apply_patch",
                "apply patch",
                Some(json!({ "type": "object" })),
            ),
            ToolSource::Local,
            ToolPayloadKind::Function,
            ToolParallelism::SerialOnly,
            true,
            ToolGovernanceSpec {
                default_approval: ToolApprovalRequirement::Skip,
                preferred_sandbox: ToolSandboxMode::WorkspaceWrite,
                argument_override: ToolGovernanceArgumentOverride::None,
                retry_without_sandbox: false,
            },
        );
        let plan = orchestrator
            .plan(
                &ToolInvocation::function("call-1", "apply_patch", json!({})),
                &test_context(),
                Some(&spec),
            )
            .expect("plan should succeed")
            .expect("plan should exist");
        assert_eq!(plan.approval, ToolApprovalRequirement::Skip);
        assert_eq!(plan.sandbox_mode, ToolSandboxMode::WorkspaceWrite);
    }

    #[test]
    fn test_exec_command_override_requires_approval() {
        let orchestrator = RegistryOrchestrator::from_context(&ToolContext::default());
        let plan = orchestrator
            .plan(
                &ToolInvocation::function(
                    "call-1",
                    "exec_command",
                    json!({
                        "cmd": "git pull",
                        "sandbox_permissions": "require_escalated",
                        "justification": "need to fetch remote changes"
                    }),
                ),
                &test_context(),
                Some(&exec_spec()),
            )
            .expect("plan should succeed")
            .expect("plan should exist");
        assert_eq!(plan.sandbox_mode, ToolSandboxMode::DangerFullAccess);
        assert!(matches!(
            plan.approval,
            ToolApprovalRequirement::NeedsApproval { .. }
        ));
    }
}
