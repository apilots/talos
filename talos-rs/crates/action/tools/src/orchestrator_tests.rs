use super::*;
use crate::invocation::ToolInvocation;
use crate::spec::{RegisteredToolSpec, ToolParallelism, ToolPayloadKind, ToolSource};
use serde_json::json;
use talos_config::AgentKernelConfig;
use talos_llm_provider::ToolDefinition;
use talos_tools_sandbox::{
    AbsolutePathBuf, FileSystemOperation, RequestedFileSystemAccess, ToolGovernanceSpec,
    ToolSandboxEnvironment,
};
use talos_types::UserPermissionMode;

#[derive(Clone)]
struct CountingRiskAssessor {
    calls: Arc<std::sync::atomic::AtomicUsize>,
    result: Result<talos_tools_sandbox::CommandRiskAssessment, crate::CommandRiskAssessmentError>,
}

impl crate::CommandRiskAssessor for CountingRiskAssessor {
    fn assess(
        &self,
        _input: crate::CommandRiskAssessmentInput,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        talos_tools_sandbox::CommandRiskAssessment,
                        crate::CommandRiskAssessmentError,
                    >,
                > + Send
                + '_,
        >,
    > {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let result = self.result.clone();
        Box::pin(async move { result })
    }
}

fn test_context() -> ToolDispatchContext {
    let config = AgentKernelConfig::default();
    let root =
        AbsolutePathBuf::new(std::env::current_dir().expect("current directory should resolve"))
            .expect("current directory should be absolute");
    let environment = ToolSandboxEnvironment::local(vec![root], Vec::new())
        .expect("sandbox environment should be valid");
    ToolDispatchContext::new(
        ToolContext::new_with_kernel_config("session-1", "user-1", Some(Arc::new(config)))
            .with_sandbox_environment(environment),
    )
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
            escalation: talos_tools_sandbox::ToolEscalationPolicy::RequiresApproval,
        },
    )
    .with_sandbox_operation(ToolSandboxOperationKind::SpawnProcess)
    .with_internal_name("bash")
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
fn test_subagent_tools_execute_under_configured_policy() {
    let context = test_context();
    let orchestrator = RegistryOrchestrator::from_context(context.tool_context());
    let spec = RegisteredToolSpec::new(
        ToolDefinition::new(
            "spawn_agent",
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
            &ToolInvocation::function(
                "call-3",
                "spawn_agent",
                json!({ "taskName": "review", "prompt": "count" }),
            ),
            &context,
            Some(&spec),
        )
        .expect("plan should succeed")
        .expect("plan should exist");

    assert_eq!(plan.sandbox_mode, ToolSandboxMode::UseDefault);
}

#[test]
fn configured_permission_modes_map_without_using_planner_mode() {
    let context = ToolContext::new_with_kernel_config(
        "session-1",
        "user-1",
        Some(Arc::new(AgentKernelConfig::default())),
    )
    .with_sandbox_user_permission_mode(UserPermissionMode::AskForApproval);
    assert_eq!(
        context.sandbox_user_permission_mode(),
        UserPermissionMode::AskForApproval
    );
    assert_eq!(
        configured_network_policy(&context, UserPermissionMode::AskForApproval),
        NetworkSandboxPolicy::Restricted
    );
}

#[test]
fn approve_for_me_defaults_to_enabled_network() {
    let context = ToolContext::new_with_kernel_config(
        "session-1",
        "user-1",
        Some(Arc::new(AgentKernelConfig::default())),
    )
    .with_sandbox_user_permission_mode(UserPermissionMode::ApproveForMe);
    assert_eq!(
        context.sandbox_user_permission_mode(),
        UserPermissionMode::ApproveForMe
    );
    assert_eq!(
        configured_network_policy(&context, UserPermissionMode::ApproveForMe),
        NetworkSandboxPolicy::Enabled
    );
}

#[test]
fn invocation_permission_mode_override_wins_over_config() {
    let context = ToolContext::new_with_kernel_config(
        "session-1",
        "user-1",
        Some(Arc::new(AgentKernelConfig::default())),
    )
    .with_sandbox_user_permission_mode(UserPermissionMode::AskForApproval);
    assert_eq!(
        context.sandbox_user_permission_mode(),
        UserPermissionMode::AskForApproval
    );
}

fn risk_context(
    mode: UserPermissionMode,
    assessor: Arc<dyn crate::CommandRiskAssessor>,
) -> ToolDispatchContext {
    let workspace = std::env::current_dir().expect("current directory should resolve");
    let environment = ToolSandboxEnvironment::local(
        vec![AbsolutePathBuf::new(workspace.clone()).expect("workspace should be absolute")],
        Vec::new(),
    )
    .expect("sandbox environment should be valid");
    ToolDispatchContext::new(
        ToolContext::new_with_kernel_config(
            "session-1",
            "user-1",
            Some(Arc::new(AgentKernelConfig::default())),
        )
        .with_project_root(workspace)
        .with_sandbox_environment(environment)
        .with_sandbox_user_permission_mode(mode)
        .with_command_risk_assessor(assessor),
    )
}

#[tokio::test]
async fn local_enforcing_tool_rejects_missing_sandbox_environment() {
    let mut context = ToolDispatchContext::new(
        ToolContext::new_with_kernel_config(
            "session-1",
            "user-1",
            Some(Arc::new(AgentKernelConfig::default())),
        )
        .with_project_root(std::env::current_dir().expect("current directory should resolve"))
        .with_sandbox_user_permission_mode(UserPermissionMode::AskForApproval),
    );
    let invocation = ToolInvocation::function("call-1", "Bash", json!({ "command": "pwd" }));

    let result = RegistryOrchestrator::from_context(context.tool_context())
        .prepare_execution(
            &invocation,
            &mut context,
            Some(&exec_spec()),
            Vec::new(),
            &ToolSandbox::new(),
        )
        .await;
    let Err(error) = result else {
        panic!("missing sandbox environment should fail closed");
    };

    assert!(error
        .to_string()
        .contains("sandbox environment is unavailable"));
}

#[tokio::test]
async fn local_tool_rejects_sandbox_facts_without_operation_metadata() {
    let mut context = test_context();
    let invocation = ToolInvocation::function("call-1", "Write", json!({}));
    let spec = RegisteredToolSpec::new(
        ToolDefinition::new("Write", "write file", Some(json!({ "type": "object" }))),
        ToolSource::Local,
        ToolPayloadKind::Function,
        ToolParallelism::SerialOnly,
        true,
        ToolGovernanceSpec::default(),
    );
    let root =
        AbsolutePathBuf::new(std::env::current_dir().expect("current directory should resolve"))
            .expect("current directory should be absolute");
    let access = RequestedFileSystemAccess::new(FileSystemOperation::Write, vec![root])
        .expect("filesystem access should be valid");

    let result = RegistryOrchestrator::from_context(context.tool_context())
        .prepare_execution(
            &invocation,
            &mut context,
            Some(&spec),
            vec![access],
            &ToolSandbox::new(),
        )
        .await;
    let Err(error) = result else {
        panic!("missing operation metadata should fail closed");
    };

    assert!(error.to_string().contains("without operation metadata"));
}

#[tokio::test]
async fn risk_assessor_is_called_only_for_approve_for_me_ambiguous_bash() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let assessor: Arc<dyn crate::CommandRiskAssessor> = Arc::new(CountingRiskAssessor {
        calls: calls.clone(),
        result: Ok(talos_tools_sandbox::CommandRiskAssessment {
            verdict: talos_tools_sandbox::CommandRiskVerdict::Safe,
            categories: Vec::new(),
            summary: "No command risk identified".to_string(),
        }),
    });
    let sandbox = ToolSandbox::new();
    let spec = exec_spec();

    for (mode, command, should_succeed) in [
        (UserPermissionMode::AskForApproval, "cargo test", false),
        (UserPermissionMode::FullAccess, "cargo test", true),
        (UserPermissionMode::ApproveForMe, "git status", true),
        (UserPermissionMode::ApproveForMe, "rm -rf build", false),
    ] {
        let mut context = risk_context(mode, assessor.clone());
        let invocation = ToolInvocation::function("call-1", "Bash", json!({ "command": command }));
        let result = RegistryOrchestrator::from_context(context.tool_context())
            .prepare_execution(&invocation, &mut context, Some(&spec), Vec::new(), &sandbox)
            .await
            .expect("sandbox decision should resolve");
        assert_eq!(matches!(result, ToolPreparation::Ready), should_succeed);
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);

    let mut context = risk_context(UserPermissionMode::ApproveForMe, assessor);
    let invocation = ToolInvocation::function("call-2", "Bash", json!({ "command": "cargo test" }));
    RegistryOrchestrator::from_context(context.tool_context())
        .prepare_execution(&invocation, &mut context, Some(&spec), Vec::new(), &sandbox)
        .await
        .expect("safe structured assessment should continue");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn safe_structured_assessments_do_not_create_an_approval_storm() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let assessor: Arc<dyn crate::CommandRiskAssessor> = Arc::new(CountingRiskAssessor {
        calls: calls.clone(),
        result: Ok(talos_tools_sandbox::CommandRiskAssessment {
            verdict: talos_tools_sandbox::CommandRiskVerdict::Safe,
            categories: Vec::new(),
            summary: "No command risk identified".to_string(),
        }),
    });
    let sandbox = ToolSandbox::new();
    let spec = exec_spec();

    for (index, command) in [
        "cargo test -p talos-tools",
        "python3 -m pytest tests",
        "just test -p talos-service",
    ]
    .into_iter()
    .enumerate()
    {
        let mut context = risk_context(UserPermissionMode::ApproveForMe, assessor.clone());
        let invocation = ToolInvocation::function(
            format!("call-{index}"),
            "Bash",
            json!({ "command": command }),
        );
        let preparation = RegistryOrchestrator::from_context(context.tool_context())
            .prepare_execution(&invocation, &mut context, Some(&spec), Vec::new(), &sandbox)
            .await
            .expect("structured safe assessment should resolve");
        assert!(matches!(preparation, ToolPreparation::Ready));
    }

    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 3);
}

#[tokio::test]
async fn assessor_failure_is_converted_to_approval() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let assessor: Arc<dyn crate::CommandRiskAssessor> = Arc::new(CountingRiskAssessor {
        calls: calls.clone(),
        result: Err(crate::CommandRiskAssessmentError::Unavailable {
            reason: "provider unavailable".to_string(),
        }),
    });
    let mut context = risk_context(UserPermissionMode::ApproveForMe, assessor);
    let invocation = ToolInvocation::function("call-1", "Bash", json!({ "command": "cargo test" }));

    let result = RegistryOrchestrator::from_context(context.tool_context())
        .prepare_execution(
            &invocation,
            &mut context,
            Some(&exec_spec()),
            Vec::new(),
            &ToolSandbox::new(),
        )
        .await
        .expect("sandbox decision should resolve");

    assert!(matches!(result, ToolPreparation::RequiresApproval { .. }));
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[tokio::test]
async fn safe_bash_network_escalation_reports_network_approval() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let assessor: Arc<dyn crate::CommandRiskAssessor> = Arc::new(CountingRiskAssessor {
        calls: calls.clone(),
        result: Ok(talos_tools_sandbox::CommandRiskAssessment {
            verdict: talos_tools_sandbox::CommandRiskVerdict::Safe,
            categories: Vec::new(),
            summary: "No command risk identified".to_string(),
        }),
    });
    let workspace = std::env::current_dir().expect("current directory should resolve");
    let environment = ToolSandboxEnvironment::local(
        vec![AbsolutePathBuf::new(workspace.clone()).expect("workspace should be absolute")],
        Vec::new(),
    )
    .expect("sandbox environment should be valid");
    let mut config = AgentKernelConfig::default();
    config.tools.sandbox_policy.network = talos_config::ToolSandboxNetworkConfig::Enabled;
    let mut context = ToolDispatchContext::new(
        ToolContext::new_with_kernel_config("session-1", "user-1", Some(Arc::new(config)))
            .with_project_root(workspace)
            .with_sandbox_environment(environment)
            .with_sandbox_user_permission_mode(UserPermissionMode::AskForApproval)
            .with_command_risk_assessor(assessor),
    );
    let invocation =
        ToolInvocation::function("call-network", "Bash", json!({ "command": "git status" }));

    let preparation = RegistryOrchestrator::from_context(context.tool_context())
        .prepare_execution(
            &invocation,
            &mut context,
            Some(&exec_spec()),
            Vec::new(),
            &ToolSandbox::new(),
        )
        .await
        .expect("network escalation should request approval");
    let ToolPreparation::RequiresApproval { params, .. } = preparation else {
        panic!("network escalation should request approval");
    };

    assert_eq!(params.reason, SandboxApprovalReason::NetworkAccess);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn approved_danger_full_access_binds_a_disabled_sandbox_permit() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let assessor: Arc<dyn crate::CommandRiskAssessor> = Arc::new(CountingRiskAssessor {
        calls: calls.clone(),
        result: Ok(talos_tools_sandbox::CommandRiskAssessment {
            verdict: talos_tools_sandbox::CommandRiskVerdict::Safe,
            categories: Vec::new(),
            summary: "No command risk identified".to_string(),
        }),
    });
    let mut context = risk_context(UserPermissionMode::AskForApproval, assessor);
    let invocation = ToolInvocation::function(
        "call-bypass",
        "Bash",
        json!({
            "command": "printf approved-bypass",
            "dangerously_disable_sandbox": true
        }),
    );
    let sandbox = ToolSandbox::new();
    let first = RegistryOrchestrator::from_context(context.tool_context())
        .prepare_execution(
            &invocation,
            &mut context,
            Some(&exec_spec()),
            Vec::new(),
            &sandbox,
        )
        .await
        .expect("bypass should request approval");
    let ToolPreparation::RequiresApproval {
        request_fingerprint,
        ..
    } = first
    else {
        panic!("bypass should request approval");
    };
    context
        .tool_context()
        .set_sandbox_approval_fingerprint("call-bypass", request_fingerprint);
    context.tool_context().set_user_action_response(
        "call-bypass",
        UserActionResponse::SandboxApproval(SandboxApprovalDecision::AllowOnce),
    );

    let second = RegistryOrchestrator::from_context(context.tool_context())
        .prepare_execution(
            &invocation,
            &mut context,
            Some(&exec_spec()),
            Vec::new(),
            &sandbox,
        )
        .await
        .expect("approved bypass should prepare");

    assert!(matches!(second, ToolPreparation::Ready));
    assert_eq!(
        context
            .tool_context()
            .sandbox_permit()
            .expect("approved bypass should bind a permit")
            .effective_policy()
            .sandbox,
        talos_tools_sandbox::SandboxCapability::Disabled
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}
