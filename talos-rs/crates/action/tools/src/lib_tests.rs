use super::*;
use std::collections::BTreeSet;

#[test]
fn workflow_tools_are_visible_while_compatibility_tools_stay_deferred() {
    let registry = ToolRegistry::new();
    register_core_local_tools(&registry);
    register_collaboration_tools(&registry);

    let visible_names = registry
        .get_tool_definitions()
        .into_iter()
        .map(|definition| definition.name)
        .collect::<BTreeSet<_>>();

    assert!(visible_names.contains("Agent"));
    assert!(visible_names.contains("TodoWrite"));
    assert!(visible_names.contains("EnterPlanMode"));
    assert!(visible_names.contains("ExitPlanMode"));
    assert!(visible_names.contains("AskUserQuestion"));
    assert!(visible_names.contains("Bash"));
    assert!(visible_names.contains("TaskOutput"));
    assert!(visible_names.contains("TaskStop"));

    assert!(!visible_names.contains("task_output"));
    assert!(!visible_names.contains("bash"));
    assert!(!visible_names.contains("Skill"));
}
