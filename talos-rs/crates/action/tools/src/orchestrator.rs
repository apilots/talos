use serde_json::Value;
use talos_config::{ToolApprovalPolicy, ToolSandboxModeConfig};
use talos_error::{TalosError, TalosResult};
use talos_tools_sandbox::{
    ToolApprovalRequirement, ToolExecutionPlan, ToolOrchestrator, ToolOrchestratorConfig,
    ToolPlanError, ToolSandboxMode,
};

use crate::context::ToolContext;
use crate::invocation::{ToolCallPayload, ToolDispatchContext, ToolInvocation};
use crate::output::ToolDispatchOutcome;

pub struct RegistryOrchestrator {
    inner: ToolOrchestrator,
}

impl RegistryOrchestrator {
    pub fn from_context(context: &ToolContext) -> Self {
        let default_sandbox_mode = context
            .kernel_config()
            .map(|config| map_sandbox_mode(&config.tools.sandbox_policy.default_mode))
            .unwrap_or(ToolSandboxMode::UseDefault);
        Self {
            inner: ToolOrchestrator::new(ToolOrchestratorConfig {
                default_sandbox_mode,
            }),
        }
    }

    pub fn plan(
        &self,
        invocation: &ToolInvocation,
        context: &ToolDispatchContext,
    ) -> TalosResult<Option<ToolExecutionPlan>> {
        if context.tool_context().pre_approved() {
            return Ok(None);
        }

        let Some(config) = context.tool_context().kernel_config() else {
            return Ok(None);
        };

        match invocation.tool_name.as_str() {
            "exec_command" => {
                let ToolCallPayload::Function { arguments } = &invocation.payload else {
                    return Err(TalosError::ToolError(
                        "exec_command requires function payload".to_string(),
                    ));
                };
                let requirement = match arguments
                    .get("sandbox_permissions")
                    .and_then(Value::as_str)
                    .unwrap_or("use_default")
                {
                    "require_escalated" => {
                        let reason = arguments
                            .get("justification")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .or_else(|| {
                                Some("exec_command requested escalated permissions".to_string())
                            });
                        match config.tools.approval_policy {
                            ToolApprovalPolicy::Never => {
                                ToolApprovalRequirement::Forbidden {
                                    reason: reason.unwrap_or_else(|| {
                                        "exec_command escalation is disabled".to_string()
                                    }),
                                }
                            }
                            ToolApprovalPolicy::OnRequest => {
                                ToolApprovalRequirement::NeedsApproval { reason }
                            }
                        }
                    }
                    _ => ToolApprovalRequirement::Skip,
                };
                Ok(Some(ToolExecutionPlan {
                    approval: requirement,
                    sandbox_mode: if arguments
                        .get("sandbox_permissions")
                        .and_then(Value::as_str)
                        == Some("require_escalated")
                    {
                        ToolSandboxMode::DangerFullAccess
                    } else {
                        self.inner.config().default_sandbox_mode
                    },
                    retry_without_sandbox: true,
                }))
            }
            "apply_patch" => Ok(Some(ToolExecutionPlan {
                approval: ToolApprovalRequirement::Skip,
                sandbox_mode: ToolSandboxMode::WorkspaceWrite,
                retry_without_sandbox: false,
            })),
            _ => Ok(None),
        }
    }

    pub fn resolve_pending(
        &self,
        invocation: &ToolInvocation,
        context: &ToolDispatchContext,
    ) -> TalosResult<Option<ToolDispatchOutcome>> {
        let Some(plan) = self.plan(invocation, context)? else {
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
}

fn map_sandbox_mode(mode: &ToolSandboxModeConfig) -> ToolSandboxMode {
    match mode {
        ToolSandboxModeConfig::UseDefault => ToolSandboxMode::UseDefault,
        ToolSandboxModeConfig::ReadOnly => ToolSandboxMode::ReadOnly,
        ToolSandboxModeConfig::WorkspaceWrite => ToolSandboxMode::WorkspaceWrite,
        ToolSandboxModeConfig::DangerFullAccess => ToolSandboxMode::DangerFullAccess,
    }
}

fn approval_prompt(requirement: &ToolApprovalRequirement) -> String {
    match requirement {
        ToolApprovalRequirement::NeedsApproval { reason } => reason
            .as_ref()
            .map(|value| format!("tool approval is required: {value}"))
            .unwrap_or_else(|| "tool approval is required".to_string()),
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
