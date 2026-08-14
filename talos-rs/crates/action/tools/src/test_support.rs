use std::collections::BTreeSet;

use talos_tools_sandbox::{
    AbsolutePathBuf, FileSystemPermissionLevel, NetworkSandboxPolicy, RequestedToolPermissions,
    ToolGovernanceConstraints, ToolInvocationIdentity, ToolSandbox, ToolSandboxDecision,
    ToolSandboxEnvironment, ToolSandboxOperationKind, ToolSandboxRequest,
};
use talos_types::UserPermissionMode;

use crate::ToolContext;

pub(crate) fn full_access_process_context(mut context: ToolContext) -> ToolContext {
    bind_full_access_permit(
        &mut context,
        ToolSandboxOperationKind::SpawnProcess,
        "test-process",
    );
    context
}

pub(crate) fn full_access_file_context(mut context: ToolContext) -> ToolContext {
    bind_full_access_permit(
        &mut context,
        ToolSandboxOperationKind::FileSystem,
        "test-file-system",
    );
    context
}

fn bind_full_access_permit(
    context: &mut ToolContext,
    operation: ToolSandboxOperationKind,
    tool_name: &str,
) {
    let root = context
        .project_root()
        .expect("test process context requires a project root");
    let root = AbsolutePathBuf::new(
        std::fs::canonicalize(root).expect("test project root must canonicalize"),
    )
    .expect("test project root must be absolute");
    let request = ToolSandboxRequest {
        invocation: ToolInvocationIdentity::new("test-call", tool_name),
        user_mode: UserPermissionMode::FullAccess,
        governance: ToolGovernanceConstraints::default(),
        requested_permissions: RequestedToolPermissions::new(
            FileSystemPermissionLevel::FullAccess,
            NetworkSandboxPolicy::Enabled,
            None,
            BTreeSet::from([operation]),
        )
        .expect("test process permissions must be valid"),
        approval_grant: None,
        command_risk_assessment: None,
        environment: ToolSandboxEnvironment::local(vec![root], Vec::new())
            .expect("test sandbox environment must be valid"),
    };
    let ToolSandboxDecision::Allowed(permit) = ToolSandbox::new()
        .evaluate(request)
        .expect("test sandbox request must evaluate")
    else {
        panic!("full access test request must be allowed");
    };
    context.bind_sandbox_permit(permit);
}
