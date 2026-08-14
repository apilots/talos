use super::*;

#[derive(Clone, Copy, Debug, Default)]
pub struct EffectiveToolPermissionPolicyResolver;

impl EffectiveToolPermissionPolicyResolver {
    pub fn resolve(&self, request: ToolSandboxRequest) -> ToolSandboxDecision {
        match self.precheck(&request) {
            PolicyPrecheck::Allowed(policy) => match request.environment.location {
                ToolExecutionLocation::Local => {
                    ToolSandboxDecision::Allowed(ToolSandboxPermit::new(
                        request.invocation,
                        request.requested_permissions.operations,
                        policy,
                    ))
                }
                ToolExecutionLocation::External(enforcement) => {
                    ToolSandboxDecision::External(enforcement)
                }
            },
            PolicyPrecheck::RequiresApproval(approval) => {
                ToolSandboxDecision::RequiresApproval(approval)
            }
            PolicyPrecheck::NeedsRiskAssessment => ToolSandboxDecision::NeedsRiskAssessment,
            PolicyPrecheck::Denied(denial) => ToolSandboxDecision::Denied(denial),
        }
    }

    pub fn precheck(&self, request: &ToolSandboxRequest) -> PolicyPrecheck {
        if let Some(maximum) = request.governance.maximum_file_system {
            if request.requested_permissions.file_system > maximum {
                return PolicyPrecheck::Denied(ToolSandboxDenial::GovernanceConstraint {
                    reason: "requested filesystem permission exceeds governance maximum"
                        .to_string(),
                });
            }
        }
        if let Some(maximum) = request.governance.maximum_network {
            if request.requested_permissions.network > maximum {
                return PolicyPrecheck::Denied(ToolSandboxDenial::GovernanceConstraint {
                    reason: "requested network permission exceeds governance maximum".to_string(),
                });
            }
        }
        if requested_access_hits_denied_root(request) {
            return PolicyPrecheck::Denied(ToolSandboxDenial::GovernanceConstraint {
                reason: "requested filesystem path is denied by the sandbox environment"
                    .to_string(),
            });
        }

        let (approval, base_file_system, base_network) = base_policy(request.user_mode);
        let needs_approval = request.requested_permissions.file_system > base_file_system
            || request.requested_permissions.network > base_network
            || (base_file_system != FileSystemPermissionLevel::FullAccess
                && requested_access_is_outside_workspace(request));
        let has_covering_grant = request
            .approval_grant
            .as_ref()
            .is_some_and(|grant| grant.covers(&request.invocation, &request.requested_permissions));

        if needs_approval && !has_covering_grant {
            return match request.governance.escalation {
                ToolEscalationPolicy::Forbidden => {
                    PolicyPrecheck::Denied(ToolSandboxDenial::EscalationForbidden)
                }
                ToolEscalationPolicy::RequiresApproval => {
                    PolicyPrecheck::RequiresApproval(approval_request(request))
                }
            };
        }

        if !has_covering_grant {
            if let Some(decision) = command_risk_decision(request) {
                return decision;
            }
        }

        let permissions = build_permission_profile(
            request.requested_permissions.file_system,
            request.requested_permissions.network,
            &request.requested_permissions.file_system_access,
            &request.environment,
        );
        let sandbox = match &request.environment.location {
            ToolExecutionLocation::External(_) => SandboxCapability::External,
            ToolExecutionLocation::Local
                if permissions.file_system == FileSystemSandboxPolicy::DangerFullAccess
                    && permissions.network == NetworkSandboxPolicy::Enabled =>
            {
                SandboxCapability::Disabled
            }
            ToolExecutionLocation::Local => SandboxCapability::Enabled,
        };
        match EffectiveToolPermissionPolicy::new(approval, permissions, sandbox) {
            Ok(policy) => PolicyPrecheck::Allowed(policy),
            Err(_) => PolicyPrecheck::Denied(ToolSandboxDenial::GovernanceConstraint {
                reason: "effective permission policy is invalid".to_string(),
            }),
        }
    }
}

fn base_policy(
    mode: UserPermissionMode,
) -> (
    ApprovalPolicy,
    FileSystemPermissionLevel,
    NetworkSandboxPolicy,
) {
    match mode {
        UserPermissionMode::AskForApproval => (
            ApprovalPolicy {
                require_for_network: true,
                require_for_workspace_external_write: true,
            },
            FileSystemPermissionLevel::WorkspaceWrite,
            NetworkSandboxPolicy::Restricted,
        ),
        UserPermissionMode::ApproveForMe => (
            ApprovalPolicy {
                require_for_network: false,
                require_for_workspace_external_write: true,
            },
            FileSystemPermissionLevel::WorkspaceWrite,
            NetworkSandboxPolicy::Enabled,
        ),
        UserPermissionMode::FullAccess => (
            ApprovalPolicy::none(),
            FileSystemPermissionLevel::FullAccess,
            NetworkSandboxPolicy::Enabled,
        ),
    }
}

fn command_risk_decision(request: &ToolSandboxRequest) -> Option<PolicyPrecheck> {
    use crate::{CommandRisk, CommandRiskVerdict};

    let risk = request.requested_permissions.command_risk?;
    match (request.user_mode, risk) {
        (UserPermissionMode::FullAccess, _) | (_, CommandRisk::Safe) => None,
        (UserPermissionMode::AskForApproval, CommandRisk::Dangerous | CommandRisk::Ambiguous)
        | (UserPermissionMode::ApproveForMe, CommandRisk::Dangerous) => {
            Some(approval_or_denial(request))
        }
        (UserPermissionMode::ApproveForMe, CommandRisk::Ambiguous) => {
            match request
                .command_risk_assessment
                .as_ref()
                .map(|item| item.verdict)
            {
                Some(CommandRiskVerdict::Safe) => None,
                Some(CommandRiskVerdict::Risky | CommandRiskVerdict::Unknown) => {
                    Some(approval_or_denial(request))
                }
                None => Some(PolicyPrecheck::NeedsRiskAssessment),
            }
        }
    }
}

fn approval_or_denial(request: &ToolSandboxRequest) -> PolicyPrecheck {
    match request.governance.escalation {
        ToolEscalationPolicy::Forbidden => {
            PolicyPrecheck::Denied(ToolSandboxDenial::EscalationForbidden)
        }
        ToolEscalationPolicy::RequiresApproval => {
            PolicyPrecheck::RequiresApproval(approval_request(request))
        }
    }
}

fn build_permission_profile(
    file_system: FileSystemPermissionLevel,
    network: NetworkSandboxPolicy,
    file_system_access: &[RequestedFileSystemAccess],
    environment: &ToolSandboxEnvironment,
) -> ToolPermissionProfile {
    let mut additional_readable = Vec::new();
    let mut additional_writable = Vec::new();
    for access in file_system_access {
        additional_readable.extend(access.paths.iter().cloned());
        if access.operation == FileSystemOperation::Write {
            additional_writable.extend(access.paths.iter().cloned());
        }
    }
    let file_system = match file_system {
        FileSystemPermissionLevel::ReadOnly => FileSystemSandboxPolicy::ReadOnly(FileSystemScope {
            readable_roots: compact_paths(
                environment
                    .readable_roots
                    .iter()
                    .cloned()
                    .chain(additional_readable)
                    .collect(),
            ),
            denied_roots: environment.denied_roots.clone(),
        }),
        FileSystemPermissionLevel::WorkspaceWrite => {
            FileSystemSandboxPolicy::WorkspaceWrite(WorkspaceWritePolicy {
                readable_roots: compact_paths(
                    environment
                        .readable_roots
                        .iter()
                        .cloned()
                        .chain(additional_readable)
                        .collect(),
                ),
                writable_roots: compact_paths(
                    environment
                        .workspace_roots
                        .iter()
                        .cloned()
                        .chain(additional_writable)
                        .collect(),
                ),
                denied_roots: environment.denied_roots.clone(),
            })
        }
        FileSystemPermissionLevel::FullAccess => FileSystemSandboxPolicy::DangerFullAccess,
    };
    ToolPermissionProfile {
        file_system,
        network,
    }
}

fn requested_access_is_outside_workspace(request: &ToolSandboxRequest) -> bool {
    request
        .requested_permissions
        .file_system_access
        .iter()
        .flat_map(|access| &access.paths)
        .any(|path| {
            !request
                .environment
                .workspace_roots
                .iter()
                .any(|root| path.as_path().starts_with(root.as_path()))
        })
}

fn requested_access_hits_denied_root(request: &ToolSandboxRequest) -> bool {
    request
        .requested_permissions
        .file_system_access
        .iter()
        .flat_map(|access| &access.paths)
        .any(|path| {
            request
                .environment
                .denied_roots
                .iter()
                .any(|root| path.as_path().starts_with(root.as_path()))
        })
}
