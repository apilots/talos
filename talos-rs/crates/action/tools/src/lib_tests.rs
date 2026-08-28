use super::*;
use std::collections::BTreeSet;

#[test]
fn workflow_tools_are_visible_while_compatibility_tools_stay_deferred() {
    let registry = ToolRegistry::new();
    register_core_local_tools(&registry);
    register_subagent_tools(&registry);

    let visible_names = registry
        .get_tool_definitions()
        .into_iter()
        .map(|definition| definition.name)
        .collect::<BTreeSet<_>>();

    for name in [
        "spawn_agent",
        "send_message",
        "wait_agent",
        "interrupt_agent",
        "followup_task",
        "list_agents",
    ] {
        assert!(visible_names.contains(name));
    }
    assert!(visible_names.contains("TodoWrite"));
    assert!(visible_names.contains("AskUserQuestion"));
    assert!(visible_names.contains("Bash"));

    assert!(!visible_names.contains("bash"));
    assert!(!visible_names.contains("Skill"));
}

#[test]
fn collaboration_tools_are_selected_independently_of_prompt_language() {
    let registry = ToolRegistry::new();
    register_subagent_tools(&registry);

    for prompt in [
        "并行审查这份实现",
        "delegate this review",
        "review the implementation",
    ] {
        let names = registry
            .select_tools(&ToolSelectionRequest::default().with_user_input(prompt))
            .model_definitions()
            .into_iter()
            .map(|definition| definition.name)
            .collect::<BTreeSet<_>>();
        assert_eq!(names.len(), 6);
        assert!(names.contains("spawn_agent"));
        assert!(names.contains("send_message"));
        assert!(names.contains("list_agents"));
    }
}

#[test]
fn user_workspace_memory_update_is_always_available_to_the_model() {
    let registry = ToolRegistry::new();
    register_core_local_tools(&registry);

    let names = registry
        .select_tools(&ToolSelectionRequest::default().with_user_input("ordinary request"))
        .model_definitions()
        .into_iter()
        .map(|definition| definition.name)
        .collect::<BTreeSet<_>>();

    assert!(names.contains("UserWorkspaceMemoryUpdate"));
}
