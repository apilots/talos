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
