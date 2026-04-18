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
                    let reason = arguments
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| {
                            arguments
                                .get("justification")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                        })
                        .or_else(|| {
                            Some(format!(
                                "{} requested escalated permissions",
                                invocation.tool_name
                            ))
                        });
                    match approval_policy {
                        ToolApprovalPolicy::Never => {
                            governance.default_approval = ToolApprovalRequirement::Forbidden {
                                reason: reason.clone().unwrap_or_else(|| {
                                    format!("{} escalation is disabled", invocation.tool_name)
                                }),
                            };
                            policy_reason = Some(GovernanceDecisionReason::GlobalPolicy {
                                policy: ToolApprovalPolicySnapshot::Never,
                            });
                        }
                        ToolApprovalPolicy::Always => {
                            governance.default_approval =
                                ToolApprovalRequirement::NeedsApproval { reason };
                            policy_reason = Some(GovernanceDecisionReason::GlobalPolicy {
                                policy: ToolApprovalPolicySnapshot::Always,
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

        if requires_session_approval(spec) {
            match approval_policy {
                ToolApprovalPolicy::Always
                    if matches!(governance.default_approval, ToolApprovalRequirement::Skip) =>
                {
                    governance.default_approval = ToolApprovalRequirement::NeedsApproval {
                        reason: Some(format!(
                            "{} requires approval in this session",
                            spec.definition.name
                        )),
                    };
                    policy_reason = Some(GovernanceDecisionReason::GlobalPolicy {
                        policy: ToolApprovalPolicySnapshot::Always,
                    });
                }
                ToolApprovalPolicy::Never
                    if matches!(
                        governance.default_approval,
                        ToolApprovalRequirement::NeedsApproval { .. }
                    ) =>
                {
                    governance.default_approval = ToolApprovalRequirement::Skip;
                    policy_reason = Some(GovernanceDecisionReason::GlobalPolicy {
                        policy: ToolApprovalPolicySnapshot::Never,
                    });
                }
                _ => {}
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

fn requires_session_approval(spec: &RegisteredToolSpec) -> bool {
    matches!(
        spec.source,
        crate::spec::ToolSource::Collaboration | crate::spec::ToolSource::Skill
    ) || spec.behavior_flags.destructive
        || spec.mutating
        || spec.behavior_flags.requires_user_interaction
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
                "Bash",
                "run shell command",
                Some(json!({ "type": "object" })),
            ),
            ToolSource::Local,
            ToolPayloadKind::Function,
            ToolParallelism::SerialOnly,
            true,
            ToolGovernanceSpec {
                default_approval: ToolApprovalRequirement::Skip,
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
        assert_eq!(plan.approval, ToolApprovalRequirement::Skip);
        assert_eq!(plan.sandbox_mode, ToolSandboxMode::UseDefault);
    }

    #[test]
    fn test_bash_command_override_requires_approval() {
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
        assert!(matches!(
            plan.approval,
            ToolApprovalRequirement::NeedsApproval { .. }
        ));
    }

    #[test]
    fn test_always_policy_requires_approval_for_mutating_tools() {
        let mut config = AgentKernelConfig::default();
        config.tools.approval_policy = ToolApprovalPolicy::Always;
        let context = ToolDispatchContext::new(ToolContext::new_with_kernel_config(
            "session-1",
            "user-1",
            Some(Arc::new(config)),
        ));
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

        assert!(matches!(
            plan.approval,
            ToolApprovalRequirement::NeedsApproval { .. }
        ));
    }

    #[test]
    fn test_never_policy_skips_approval_for_collaboration_tools() {
        let mut config = AgentKernelConfig::default();
        config.tools.approval_policy = ToolApprovalPolicy::Never;
        let context = ToolDispatchContext::new(ToolContext::new_with_kernel_config(
            "session-1",
            "user-1",
            Some(Arc::new(config)),
        ));
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
            ToolGovernanceSpec {
                default_approval: ToolApprovalRequirement::NeedsApproval {
                    reason: Some("tool spawns sub-agent work and requires approval".to_string()),
                },
                preferred_sandbox: ToolSandboxMode::UseDefault,
                argument_override: ToolGovernanceArgumentOverride::None,
                retry_without_sandbox: false,
            },
        );

        let plan = orchestrator
            .plan(
                &ToolInvocation::function("call-3", "Agent", json!({ "description": "count" })),
                &context,
                Some(&spec),
            )
            .expect("plan should succeed")
            .expect("plan should exist");

        assert_eq!(plan.approval, ToolApprovalRequirement::Skip);
    }
}
