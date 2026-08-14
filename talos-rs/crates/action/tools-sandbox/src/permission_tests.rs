use std::collections::BTreeSet;
use std::env;
use talos_types::UserPermissionMode;

use super::*;
use crate::{CommandRisk, CommandRiskAssessment, CommandRiskCategory, CommandRiskVerdict};

fn root(name: &str) -> AbsolutePathBuf {
    AbsolutePathBuf::new(
        env::current_dir()
            .expect("current directory must resolve")
            .join(name),
    )
    .expect("test root must be absolute")
}

fn identity(call_id: &str) -> ToolInvocationIdentity {
    ToolInvocationIdentity::new(call_id, "Read")
}

fn environment() -> ToolSandboxEnvironment {
    ToolSandboxEnvironment::local(vec![root("workspace")], vec![root("denied")])
        .expect("test environment must be valid")
}

fn request(
    call_id: &str,
    user_mode: UserPermissionMode,
    file_system: FileSystemPermissionLevel,
    network: NetworkSandboxPolicy,
) -> ToolSandboxRequest {
    ToolSandboxRequest {
        invocation: identity(call_id),
        user_mode,
        governance: ToolGovernanceConstraints::default(),
        requested_permissions: RequestedToolPermissions::new(
            file_system,
            network,
            None,
            BTreeSet::from([ToolSandboxOperationKind::FileSystem]),
        )
        .expect("requested permissions must be valid"),
        approval_grant: None,
        command_risk_assessment: None,
        environment: environment(),
    }
}

fn allowed_policy(decision: ToolSandboxDecision) -> EffectiveToolPermissionPolicy {
    let ToolSandboxDecision::Allowed(permit) = decision else {
        panic!("decision must be allowed");
    };
    permit.effective_policy().clone()
}

#[test]
fn user_modes_map_to_expected_effective_policies() {
    let resolver = EffectiveToolPermissionPolicyResolver;

    let ask = allowed_policy(resolver.resolve(request(
        "ask",
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::WorkspaceWrite,
        NetworkSandboxPolicy::Restricted,
    )));
    assert_eq!(
        ask,
        EffectiveToolPermissionPolicy::new(
            ApprovalPolicy {
                require_for_network: true,
                require_for_workspace_external_write: true,
            },
            ToolPermissionProfile {
                file_system: FileSystemSandboxPolicy::WorkspaceWrite(WorkspaceWritePolicy {
                    readable_roots: vec![root("workspace")],
                    writable_roots: vec![root("workspace")],
                    denied_roots: vec![root("denied")],
                }),
                network: NetworkSandboxPolicy::Restricted,
            },
            SandboxCapability::Enabled,
        )
        .expect("policy must be valid")
    );

    let approve = allowed_policy(resolver.resolve(request(
        "approve",
        UserPermissionMode::ApproveForMe,
        FileSystemPermissionLevel::WorkspaceWrite,
        NetworkSandboxPolicy::Enabled,
    )));
    assert_eq!(approve.permissions.network, NetworkSandboxPolicy::Enabled);
    assert_eq!(approve.sandbox, SandboxCapability::Enabled);

    let full = allowed_policy(resolver.resolve(request(
        "full",
        UserPermissionMode::FullAccess,
        FileSystemPermissionLevel::FullAccess,
        NetworkSandboxPolicy::Enabled,
    )));
    assert_eq!(
        full.permissions.file_system,
        FileSystemSandboxPolicy::DangerFullAccess
    );
    assert_eq!(full.sandbox, SandboxCapability::Disabled);
}

#[test]
fn workspace_write_keeps_runtime_roots_read_only() {
    let resolver = EffectiveToolPermissionPolicyResolver;
    let mut runtime_request = request(
        "runtime-roots",
        UserPermissionMode::ApproveForMe,
        FileSystemPermissionLevel::WorkspaceWrite,
        NetworkSandboxPolicy::Enabled,
    );
    runtime_request.environment = runtime_request
        .environment
        .with_readable_roots(vec![root("home"), root("toolchain")]);

    let policy = allowed_policy(resolver.resolve(runtime_request));
    assert_eq!(
        policy.permissions.file_system,
        FileSystemSandboxPolicy::WorkspaceWrite(WorkspaceWritePolicy {
            readable_roots: vec![root("home"), root("toolchain"), root("workspace")],
            writable_roots: vec![root("workspace")],
            denied_roots: vec![root("denied")],
        })
    );
}

#[test]
fn ask_mode_requires_matching_approval_for_network() {
    let resolver = EffectiveToolPermissionPolicyResolver;
    let mut pending = request(
        "call-1",
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::WorkspaceWrite,
        NetworkSandboxPolicy::Enabled,
    );
    assert!(matches!(
        resolver.resolve(pending.clone()),
        ToolSandboxDecision::RequiresApproval(ToolApprovalRequest { .. })
    ));

    pending.approval_grant = Some(ToolApprovalGrant::new(
        identity("different-call"),
        FileSystemPermissionLevel::WorkspaceWrite,
        NetworkSandboxPolicy::Enabled,
        BTreeSet::from([ToolSandboxOperationKind::FileSystem]),
        Vec::new(),
    ));
    assert!(matches!(
        resolver.resolve(pending.clone()),
        ToolSandboxDecision::RequiresApproval(_)
    ));

    pending.approval_grant = Some(ToolApprovalGrant::new(
        identity("call-1"),
        FileSystemPermissionLevel::WorkspaceWrite,
        NetworkSandboxPolicy::Enabled,
        BTreeSet::from([ToolSandboxOperationKind::FileSystem]),
        Vec::new(),
    ));
    assert_eq!(
        allowed_policy(resolver.resolve(pending))
            .permissions
            .network,
        NetworkSandboxPolicy::Enabled
    );
}

#[test]
fn approval_fingerprint_binds_allow_once_to_the_exact_request_scope() {
    let original = request(
        "call-1",
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::WorkspaceWrite,
        NetworkSandboxPolicy::Enabled,
    );
    let same_scope = approval_request(&original);
    let original_approval = approval_request(&original);
    assert_eq!(original_approval.fingerprint(), same_scope.fingerprint());

    let mut changed = original.clone();
    changed.requested_permissions.file_system = FileSystemPermissionLevel::FullAccess;
    let changed_approval = approval_request(&changed);
    assert_ne!(
        original_approval.fingerprint(),
        changed_approval.fingerprint()
    );
    assert_eq!(
        changed_approval.allow_once(original_approval.fingerprint()),
        Err(ToolPermissionError::ApprovalScopeMismatch)
    );

    let mut approved = original.clone();
    approved.approval_grant = Some(
        original_approval
            .allow_once(original_approval.fingerprint())
            .expect("matching scope should create a one-time grant"),
    );
    assert!(matches!(
        EffectiveToolPermissionPolicyResolver.resolve(approved),
        ToolSandboxDecision::Allowed(_)
    ));

    let mut changed_invocation = original;
    changed_invocation.invocation =
        identity("call-1").with_scope_digest("different-tool-arguments");
    assert_ne!(
        original_approval.fingerprint(),
        approval_request(&changed_invocation).fingerprint()
    );
}

#[test]
fn approved_full_access_turns_a_restricted_user_mode_into_noop() {
    let resolver = EffectiveToolPermissionPolicyResolver;
    let mut approved = request(
        "call-1",
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::FullAccess,
        NetworkSandboxPolicy::Enabled,
    );
    approved.approval_grant = Some(ToolApprovalGrant::new(
        identity("call-1"),
        FileSystemPermissionLevel::FullAccess,
        NetworkSandboxPolicy::Enabled,
        BTreeSet::from([ToolSandboxOperationKind::FileSystem]),
        Vec::new(),
    ));

    assert_eq!(
        allowed_policy(resolver.resolve(approved)).sandbox,
        SandboxCapability::Disabled
    );
}

#[test]
fn governance_can_narrow_but_cannot_grant_full_access() {
    let resolver = EffectiveToolPermissionPolicyResolver;
    let mut denied = request(
        "call-1",
        UserPermissionMode::FullAccess,
        FileSystemPermissionLevel::FullAccess,
        NetworkSandboxPolicy::Enabled,
    );
    denied.governance.maximum_file_system = Some(FileSystemPermissionLevel::ReadOnly);
    assert_eq!(
        resolver.resolve(denied),
        ToolSandboxDecision::Denied(ToolSandboxDenial::GovernanceConstraint {
            reason: "requested filesystem permission exceeds governance maximum".to_string(),
        })
    );

    let mut request_from_restricted = request(
        "call-2",
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::FullAccess,
        NetworkSandboxPolicy::Enabled,
    );
    request_from_restricted.governance.escalation = ToolEscalationPolicy::Forbidden;
    assert!(matches!(
        resolver.resolve(request_from_restricted),
        ToolSandboxDecision::Denied(ToolSandboxDenial::EscalationForbidden)
    ));
}

#[test]
fn command_risk_matrix_is_owned_by_the_permission_resolver() {
    let resolver = EffectiveToolPermissionPolicyResolver;
    let mut request = request(
        "call-1",
        UserPermissionMode::ApproveForMe,
        FileSystemPermissionLevel::WorkspaceWrite,
        NetworkSandboxPolicy::Enabled,
    );
    request.governance.escalation = ToolEscalationPolicy::RequiresApproval;

    request.requested_permissions.command_risk = Some(CommandRisk::Safe);
    assert!(matches!(
        resolver.resolve(request.clone()),
        ToolSandboxDecision::Allowed(_)
    ));

    request.requested_permissions.command_risk = Some(CommandRisk::Dangerous);
    assert!(matches!(
        resolver.resolve(request.clone()),
        ToolSandboxDecision::RequiresApproval(_)
    ));

    request.requested_permissions.command_risk = Some(CommandRisk::Ambiguous);
    assert_eq!(
        resolver.resolve(request.clone()),
        ToolSandboxDecision::NeedsRiskAssessment
    );

    request.command_risk_assessment = Some(CommandRiskAssessment {
        verdict: CommandRiskVerdict::Safe,
        categories: Vec::new(),
        summary: "No command risk identified".to_string(),
    });
    assert!(matches!(
        resolver.resolve(request.clone()),
        ToolSandboxDecision::Allowed(_)
    ));

    request.command_risk_assessment = Some(CommandRiskAssessment {
        verdict: CommandRiskVerdict::Unknown,
        categories: vec![CommandRiskCategory::Unknown],
        summary: "Command effect is unclear".to_string(),
    });

    assert!(matches!(
        resolver.resolve(request),
        ToolSandboxDecision::RequiresApproval(_)
    ));
}

#[test]
fn ask_for_approval_never_requests_model_risk_assessment() {
    let resolver = EffectiveToolPermissionPolicyResolver;
    let mut request = request(
        "call-1",
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::WorkspaceWrite,
        NetworkSandboxPolicy::Restricted,
    );
    request.governance.escalation = ToolEscalationPolicy::RequiresApproval;
    request.requested_permissions.command_risk = Some(CommandRisk::Ambiguous);

    assert!(matches!(
        resolver.resolve(request),
        ToolSandboxDecision::RequiresApproval(_)
    ));
}

#[test]
fn full_access_ignores_command_risk_and_assessment() {
    let resolver = EffectiveToolPermissionPolicyResolver;
    let mut request = request(
        "call-1",
        UserPermissionMode::FullAccess,
        FileSystemPermissionLevel::FullAccess,
        NetworkSandboxPolicy::Enabled,
    );
    request.requested_permissions.command_risk = Some(CommandRisk::Dangerous);

    assert!(matches!(
        resolver.resolve(request),
        ToolSandboxDecision::Allowed(_)
    ));
}

#[test]
fn permission_escalation_precedes_safe_model_assessment() {
    let resolver = EffectiveToolPermissionPolicyResolver;
    let mut request = request(
        "call-1",
        UserPermissionMode::ApproveForMe,
        FileSystemPermissionLevel::FullAccess,
        NetworkSandboxPolicy::Enabled,
    );
    request.governance.escalation = ToolEscalationPolicy::RequiresApproval;
    request.requested_permissions.command_risk = Some(CommandRisk::Ambiguous);
    request.command_risk_assessment = Some(CommandRiskAssessment {
        verdict: CommandRiskVerdict::Safe,
        categories: Vec::new(),
        summary: "No command risk identified".to_string(),
    });

    assert!(matches!(
        resolver.resolve(request),
        ToolSandboxDecision::RequiresApproval(_)
    ));
}

#[test]
fn mismatched_grant_cannot_bypass_command_risk() {
    let resolver = EffectiveToolPermissionPolicyResolver;
    let mut request = request(
        "call-1",
        UserPermissionMode::ApproveForMe,
        FileSystemPermissionLevel::WorkspaceWrite,
        NetworkSandboxPolicy::Enabled,
    );
    request.governance.escalation = ToolEscalationPolicy::RequiresApproval;
    request.requested_permissions.command_risk = Some(CommandRisk::Dangerous);
    request.approval_grant = Some(ToolApprovalGrant::new(
        identity("other-call"),
        FileSystemPermissionLevel::WorkspaceWrite,
        NetworkSandboxPolicy::Enabled,
        BTreeSet::from([ToolSandboxOperationKind::FileSystem]),
        Vec::new(),
    ));

    assert!(matches!(
        resolver.resolve(request),
        ToolSandboxDecision::RequiresApproval(_)
    ));
}

#[test]
fn outside_file_access_requires_an_exact_matching_grant() {
    let resolver = EffectiveToolPermissionPolicyResolver;
    let outside = root("outside/file.txt");
    let mut pending = request(
        "call-1",
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::ReadOnly,
        NetworkSandboxPolicy::Restricted,
    );
    pending.requested_permissions.file_system_access =
        vec![
            RequestedFileSystemAccess::new(FileSystemOperation::Read, vec![outside.clone()])
                .expect("filesystem access should be valid"),
        ];

    let ToolSandboxDecision::RequiresApproval(approval) = resolver.resolve(pending.clone()) else {
        panic!("outside access must require approval");
    };
    pending.approval_grant = Some(
        approval
            .allow_once(approval.fingerprint())
            .expect("matching approval should create a grant"),
    );

    let ToolSandboxDecision::Allowed(permit) = resolver.resolve(pending) else {
        panic!("matching approval must allow the request");
    };
    let FileSystemSandboxPolicy::ReadOnly(scope) =
        &permit.effective_policy().permissions.file_system
    else {
        panic!("read request should keep a read-only policy");
    };
    assert!(scope.readable_roots.contains(&outside));
}

#[test]
fn file_access_grant_does_not_cover_another_path() {
    let resolver = EffectiveToolPermissionPolicyResolver;
    let mut first = request(
        "call-1",
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::ReadOnly,
        NetworkSandboxPolicy::Restricted,
    );
    first.requested_permissions.file_system_access = vec![RequestedFileSystemAccess::new(
        FileSystemOperation::Read,
        vec![root("outside/one.txt")],
    )
    .expect("filesystem access should be valid")];
    let ToolSandboxDecision::RequiresApproval(approval) = resolver.resolve(first) else {
        panic!("outside access must require approval");
    };

    let mut second = request(
        "call-1",
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::ReadOnly,
        NetworkSandboxPolicy::Restricted,
    );
    second.requested_permissions.file_system_access = vec![RequestedFileSystemAccess::new(
        FileSystemOperation::Read,
        vec![root("outside/two.txt")],
    )
    .expect("filesystem access should be valid")];
    second.approval_grant = Some(
        approval
            .allow_once(approval.fingerprint())
            .expect("matching approval should create a grant"),
    );

    assert!(matches!(
        resolver.resolve(second),
        ToolSandboxDecision::RequiresApproval(_)
    ));
}

#[test]
fn external_execution_returns_metadata_without_local_permit() {
    let resolver = EffectiveToolPermissionPolicyResolver;
    let mut external = request(
        "call-1",
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::WorkspaceWrite,
        NetworkSandboxPolicy::Restricted,
    );
    external.environment.location = ToolExecutionLocation::External(
        ExternalSandboxEnforcement::new(ExternalSandboxEnforcementSource::RemoteEnvironment),
    );

    assert_eq!(
        resolver.resolve(external),
        ToolSandboxDecision::External(ExternalSandboxEnforcement::new(
            ExternalSandboxEnforcementSource::RemoteEnvironment,
        ))
    );
}

#[test]
fn permit_rejects_other_invocations_and_ungranted_operations() {
    let resolver = EffectiveToolPermissionPolicyResolver;
    let decision = resolver.resolve(request(
        "call-1",
        UserPermissionMode::AskForApproval,
        FileSystemPermissionLevel::ReadOnly,
        NetworkSandboxPolicy::Restricted,
    ));
    let ToolSandboxDecision::Allowed(permit) = decision else {
        panic!("decision must be allowed");
    };

    assert_eq!(
        permit.require_operation(&identity("other"), ToolSandboxOperationKind::FileSystem),
        Err(ToolPermissionError::InvocationMismatch)
    );
    assert_eq!(
        permit.require_operation(&identity("call-1"), ToolSandboxOperationKind::SpawnProcess),
        Err(ToolPermissionError::OperationNotGranted {
            operation: ToolSandboxOperationKind::SpawnProcess,
        })
    );
}

#[test]
fn effective_policy_rejects_disabled_sandbox_without_full_access() {
    let policy = EffectiveToolPermissionPolicy::new(
        ApprovalPolicy::none(),
        ToolPermissionProfile {
            file_system: FileSystemSandboxPolicy::WorkspaceWrite(WorkspaceWritePolicy {
                readable_roots: vec![root("workspace")],
                writable_roots: vec![root("workspace")],
                denied_roots: Vec::new(),
            }),
            network: NetworkSandboxPolicy::Enabled,
        },
        SandboxCapability::Disabled,
    );

    assert_eq!(policy, Err(ToolPermissionError::InvalidEffectivePolicy));
}
