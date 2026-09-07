use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::Value;
use sha2::{Digest, Sha256};
use talos_config::{ToolSandboxModeConfig, ToolSandboxNetworkConfig};
use talos_error::{TalosError, TalosResult};
use talos_tools_sandbox::{
    FileSystemPermissionLevel, GovernanceDecisionReason, GovernanceEventSink, GovernanceOutcome,
    NetworkSandboxPolicy, RequestedFileSystemAccess, RequestedToolPermissions, ToolExecutionPlan,
    ToolGovernanceArgumentOverride, ToolGovernanceConstraints, ToolGovernanceDecision,
    ToolGovernanceSpec, ToolInvocationIdentity, ToolSandbox, ToolSandboxDecision, ToolSandboxMode,
    ToolSandboxOperationKind, ToolSandboxRequest,
};
use talos_types::{
    SandboxApprovalDecision, SandboxApprovalParams, SandboxApprovalPresentation,
    SandboxApprovalReason, SandboxFileOperation, UserActionResponse, UserPermissionMode,
};

use crate::command_risk::{
    classify_bash_command, unknown_assessment, CommandRiskAssessmentInput,
    EffectivePermissionSummary,
};
use crate::context::ToolContext;
use crate::invocation::{ToolCallPayload, ToolDispatchContext, ToolInvocation};
use crate::spec::RegisteredToolSpec;

pub struct RegistryOrchestrator {
    default_sandbox_mode: ToolSandboxMode,
    sink: Option<Arc<dyn GovernanceEventSink>>,
}

pub(crate) enum ToolPreparation {
    Ready,
    RequiresApproval {
        params: SandboxApprovalParams,
        request_fingerprint: String,
    },
    Rejected,
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
            escalation: governance.escalation,
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

    pub(crate) async fn prepare_execution(
        &self,
        invocation: &ToolInvocation,
        context: &mut ToolDispatchContext,
        spec: Option<&RegisteredToolSpec>,
        file_system_access: Vec<RequestedFileSystemAccess>,
        sandbox: &ToolSandbox,
    ) -> TalosResult<ToolPreparation> {
        let Some(spec) = spec else {
            return Ok(ToolPreparation::Ready);
        };
        let Some(operation) = spec.sandbox_operation else {
            if !file_system_access.is_empty() {
                return Err(sandbox_permission_failure(
                    "local tool returned sandbox facts without operation metadata",
                ));
            }
            return Ok(ToolPreparation::Ready);
        };
        if operation == ToolSandboxOperationKind::FileSystem && file_system_access.is_empty() {
            return Err(sandbox_permission_failure(
                "filesystem sandbox metadata is unavailable for a local tool",
            ));
        }
        let Some(plan) = self.plan(invocation, context, Some(spec))? else {
            return Err(sandbox_permission_failure(
                "sandbox policy is unavailable for a local tool",
            ));
        };
        let environment = context
            .tool_context()
            .sandbox_environment()
            .cloned()
            .ok_or_else(|| {
                sandbox_permission_failure("sandbox environment is unavailable for a local tool")
            })?;
        let cwd = context
            .tool_context()
            .project_root()
            .and_then(|root| std::fs::canonicalize(root).ok())
            .or_else(|| {
                environment
                    .workspace_roots
                    .first()
                    .map(|root| root.as_path().to_path_buf())
            })
            .ok_or_else(|| {
                sandbox_permission_failure("sandbox working directory is unavailable")
            })?;
        let user_mode = context.tool_context().sandbox_user_permission_mode();
        let file_system = match (user_mode, plan.sandbox_mode) {
            (UserPermissionMode::FullAccess, _) => FileSystemPermissionLevel::FullAccess,
            (_, ToolSandboxMode::DangerFullAccess) => FileSystemPermissionLevel::FullAccess,
            (_, ToolSandboxMode::ReadOnly) => FileSystemPermissionLevel::ReadOnly,
            (_, ToolSandboxMode::UseDefault | ToolSandboxMode::WorkspaceWrite)
                if spec.behavior_flags.read_only =>
            {
                FileSystemPermissionLevel::ReadOnly
            }
            (_, ToolSandboxMode::UseDefault | ToolSandboxMode::WorkspaceWrite) => {
                FileSystemPermissionLevel::WorkspaceWrite
            }
        };
        let network = if plan.sandbox_mode == ToolSandboxMode::DangerFullAccess {
            NetworkSandboxPolicy::Enabled
        } else {
            configured_network_policy(context.tool_context(), user_mode)
        };
        let command = bash_command(invocation, spec)?;
        let command_facts = command.map(classify_bash_command);
        let command_risk = command_facts.as_ref().map(|facts| facts.risk);
        let requested_permissions = RequestedToolPermissions::new(
            file_system,
            network,
            command_risk,
            BTreeSet::from([operation]),
        )
        .and_then(|permissions| permissions.with_file_system_access(file_system_access))
        .map_err(|error| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                error.to_string(),
            )
        })?;
        let mut request = ToolSandboxRequest {
            invocation: ToolInvocationIdentity::new(&invocation.call_id, &invocation.tool_name)
                .with_scope_digest(invocation_scope_digest(invocation)?),
            user_mode,
            governance: ToolGovernanceConstraints {
                maximum_file_system: None,
                maximum_network: None,
                escalation: plan.escalation,
            },
            requested_permissions,
            approval_grant: None,
            command_risk_assessment: None,
            environment,
        };

        if let Some(UserActionResponse::SandboxApproval(decision)) = context
            .tool_context()
            .get_user_action_response(&invocation.call_id)
        {
            match decision {
                SandboxApprovalDecision::AllowOnce => {
                    let expected = context
                        .tool_context()
                        .get_sandbox_approval_fingerprint(&invocation.call_id)
                        .ok_or_else(|| {
                            sandbox_permission_failure(
                                "sandbox approval is missing its original scope",
                            )
                        })?;
                    request.approval_grant = Some(
                        sandbox
                            .approval_request(&request)
                            .allow_once(&expected)
                            .map_err(|error| sandbox_permission_failure(error.to_string()))?,
                    );
                }
                SandboxApprovalDecision::Deny => return Ok(ToolPreparation::Rejected),
                SandboxApprovalDecision::Cancel => {
                    return Err(sandbox_permission_failure("sandbox approval was cancelled"));
                }
            }
        }

        let mut risk_summary = None;
        let mut decision = sandbox.evaluate(request.clone()).map_err(|error| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                error.to_string(),
            )
        })?;
        if decision == ToolSandboxDecision::NeedsRiskAssessment {
            let assessment = match (
                context.tool_context().command_risk_assessor(),
                command,
                command_facts.as_ref(),
            ) {
                (Some(assessor), Some(command), Some(facts)) => assessor
                    .assess(CommandRiskAssessmentInput::new(
                        command,
                        &cwd,
                        facts.clone(),
                        EffectivePermissionSummary {
                            file_system,
                            network,
                            sandbox_enabled: user_mode != UserPermissionMode::FullAccess,
                        },
                    ))
                    .await
                    .unwrap_or_else(|error| unknown_assessment(error.to_string())),
                _ => unknown_assessment("command risk assessor is unavailable"),
            };
            risk_summary = Some(assessment.summary.clone());
            let mut assessed_request = request;
            assessed_request.command_risk_assessment = Some(assessment);
            decision = sandbox.evaluate(assessed_request).map_err(|error| {
                TalosError::new(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    error.to_string(),
                )
            })?;
        }

        match decision {
            ToolSandboxDecision::Allowed(permit) => {
                context.tool_context_mut().bind_sandbox_permit(permit);
                Ok(ToolPreparation::Ready)
            }
            ToolSandboxDecision::RequiresApproval(approval) => {
                Ok(ToolPreparation::RequiresApproval {
                    params: sandbox_approval_params(
                        invocation,
                        command,
                        &cwd,
                        command_risk,
                        risk_summary.as_deref(),
                        file_system,
                        network,
                        user_mode,
                        &approval.requested_permissions.file_system_access,
                    ),
                    request_fingerprint: approval.fingerprint().to_string(),
                })
            }
            ToolSandboxDecision::NeedsRiskAssessment => Err(sandbox_permission_failure(
                "command risk assessment requires approval",
            )),
            ToolSandboxDecision::Denied(denial) => Err(sandbox_permission_failure(format!(
                "sandbox permission denied: {denial:?}"
            ))),
            ToolSandboxDecision::External(_) => Err(sandbox_permission_failure(
                "local tool resolved to external sandbox enforcement",
            )),
        }
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
                    return Err(TalosError::new(
                        talos_error::ErrorCode::ToolExecutionFailed,
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
                    governance.escalation =
                        talos_tools_sandbox::ToolEscalationPolicy::RequiresApproval;
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

#[allow(clippy::too_many_arguments)]
fn sandbox_approval_params(
    invocation: &ToolInvocation,
    command: Option<&str>,
    cwd: &std::path::Path,
    command_risk: Option<talos_tools_sandbox::CommandRisk>,
    risk_summary: Option<&str>,
    file_system: FileSystemPermissionLevel,
    network: NetworkSandboxPolicy,
    user_mode: UserPermissionMode,
    file_system_access: &[RequestedFileSystemAccess],
) -> SandboxApprovalParams {
    let reason = if file_system == FileSystemPermissionLevel::FullAccess {
        SandboxApprovalReason::SandboxBypass
    } else if !file_system_access.is_empty() {
        SandboxApprovalReason::FileSystemAccessOutsideWorkspace
    } else if user_mode == UserPermissionMode::AskForApproval
        && network == NetworkSandboxPolicy::Enabled
    {
        SandboxApprovalReason::NetworkAccess
    } else if matches!(
        command_risk,
        Some(
            talos_tools_sandbox::CommandRisk::Dangerous
                | talos_tools_sandbox::CommandRisk::Ambiguous
        )
    ) {
        SandboxApprovalReason::RiskyCommand
    } else {
        SandboxApprovalReason::FileSystemAccessOutsideWorkspace
    };
    let presentation = if !file_system_access.is_empty() {
        let operation = if file_system_access
            .iter()
            .any(|access| access.operation == talos_tools_sandbox::FileSystemOperation::Write)
        {
            SandboxFileOperation::Write
        } else {
            SandboxFileOperation::Read
        };
        let paths = file_system_access
            .iter()
            .flat_map(|access| &access.paths)
            .map(|path| path.as_path().display().to_string())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        SandboxApprovalPresentation::FileSystem { operation, paths }
    } else if let Some(command) = command {
        SandboxApprovalPresentation::Command {
            command: command.chars().take(4_096).collect(),
            cwd: cwd.to_string_lossy().chars().take(1_024).collect(),
            risk_summary: risk_summary.map(|summary| summary.chars().take(512).collect()),
        }
    } else {
        SandboxApprovalPresentation::Network {
            scope: format!("Network access for {}", invocation.tool_name),
        }
    };
    SandboxApprovalParams {
        tool_name: invocation.tool_name.clone(),
        reason,
        presentation,
        available_decisions: vec![
            SandboxApprovalDecision::AllowOnce,
            SandboxApprovalDecision::Deny,
            SandboxApprovalDecision::Cancel,
        ],
    }
}

fn bash_command<'a>(
    invocation: &'a ToolInvocation,
    spec: &RegisteredToolSpec,
) -> TalosResult<Option<&'a str>> {
    if spec.internal_name != "bash" {
        return Ok(None);
    }
    let ToolCallPayload::Function { arguments } = &invocation.payload else {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "shell tool requires function payload",
        ));
    };
    arguments
        .get("command")
        .and_then(Value::as_str)
        .map(Some)
        .ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                "shell tool requires command",
            )
        })
}

fn invocation_scope_digest(invocation: &ToolInvocation) -> TalosResult<String> {
    let encoded = serde_json::to_vec(&invocation.payload).map_err(|error| {
        TalosError::internal_with_source(
            "encode tool approval scope; technical details were withheld.",
            error,
        )
    })?;
    Ok(format!("tis_{:x}", Sha256::digest(encoded)))
}

fn sandbox_permission_failure(message: impl Into<String>) -> TalosError {
    TalosError::new(talos_error::ErrorCode::PermissionDenied, message)
}

fn map_sandbox_mode(mode: &ToolSandboxModeConfig) -> ToolSandboxMode {
    match mode {
        ToolSandboxModeConfig::UseDefault => ToolSandboxMode::UseDefault,
        ToolSandboxModeConfig::ReadOnly => ToolSandboxMode::ReadOnly,
        ToolSandboxModeConfig::WorkspaceWrite => ToolSandboxMode::WorkspaceWrite,
        ToolSandboxModeConfig::DangerFullAccess => ToolSandboxMode::DangerFullAccess,
    }
}

fn configured_network_policy(
    context: &ToolContext,
    user_mode: UserPermissionMode,
) -> NetworkSandboxPolicy {
    if user_mode == UserPermissionMode::FullAccess {
        return NetworkSandboxPolicy::Enabled;
    }
    match context
        .kernel_config()
        .map(|config| &config.tools.sandbox_policy.network)
        .unwrap_or(&ToolSandboxNetworkConfig::UseDefault)
    {
        ToolSandboxNetworkConfig::Restricted => NetworkSandboxPolicy::Restricted,
        ToolSandboxNetworkConfig::Enabled => NetworkSandboxPolicy::Enabled,
        ToolSandboxNetworkConfig::UseDefault => match user_mode {
            UserPermissionMode::AskForApproval => NetworkSandboxPolicy::Restricted,
            UserPermissionMode::ApproveForMe => NetworkSandboxPolicy::Enabled,
            UserPermissionMode::FullAccess => NetworkSandboxPolicy::Enabled,
        },
    }
}

#[cfg(test)]
#[path = "orchestrator_tests.rs"]
mod tests;
