use super::*;

pub struct FlowFactory;

/// Determine whether the PlanReAct loop should allow another replan round.
///
/// `replan_count` is the number of replan *decisions* already made this turn (one per
/// no-progress or tool-error round that continued). Both the tool-error path and the
/// no-progress path share the same `max_replans` budget so callers cannot bypass the
/// limit by mixing the two triggers.
pub(crate) fn should_replan(
    state: &PlanReActLoopState,
    last_tool_batch: Option<&GeneralTurnToolBatch>,
    policy: &talos_config::PlanReActPolicy,
) -> bool {
    // Global replan budget exhausted — never allow another round.
    if state.replan_count >= policy.max_replans {
        return false;
    }
    // Allow replan when the last tool batch had errors (model-driven replan).
    if let Some(tool_batch) = last_tool_batch {
        if tool_batch.error_count > 0 {
            return true;
        }
    }
    // Allow replan for genuine no-progress rounds within the separate no-progress budget.
    state.consecutive_no_progress_rounds < policy.max_no_progress_rounds
}

fn trim_path_token(token: &str) -> &str {
    token.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\''
                | '`'
                | ','
                | '.'
                | ':'
                | ';'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '<'
                | '>'
        )
    })
}

fn contains_local_absolute_path(input: &str) -> bool {
    input
        .split_whitespace()
        .map(trim_path_token)
        .any(|token| token.starts_with('/') && token.len() > 1)
}

fn contains_analysis_keyword(input: &str) -> bool {
    let lowered = input.to_ascii_lowercase();
    [
        "analyze",
        "analysis",
        "architecture",
        "review",
        "inspect",
        "explain",
        "summarize",
        "summary",
        "structure",
    ]
    .iter()
    .any(|keyword| lowered.contains(keyword))
        || [
            "分析", "架构", "结构", "解释", "梳理", "总结", "查看", "评审",
        ]
        .iter()
        .any(|keyword| input.contains(keyword))
}

pub(crate) fn is_local_path_analysis_request(input: &str) -> bool {
    contains_local_absolute_path(input) && contains_analysis_keyword(input)
}

fn is_file_probe_tool_name(tool_name: &str) -> bool {
    matches!(tool_name, "list_dir" | "read_file" | "grep_files")
}

fn has_file_probe_tool_call(state: &GeneralTurnLoopState) -> bool {
    state
        .tool_call_log
        .iter()
        .any(|record| is_file_probe_tool_name(&record.name))
}

fn can_use_local_directory_probe(ctx: &InvocationContext) -> bool {
    ctx.session_environment
        .as_ref()
        .map(|environment| matches!(environment.connection_mode, TalosConnectionMode::Embedded))
        .unwrap_or(false)
}

fn has_structure_probe_tool_call(state: &GeneralTurnLoopState) -> bool {
    state
        .tool_call_log
        .iter()
        .any(|record| record.name == "grep_files")
}

pub(crate) fn should_require_local_path_file_probe(
    ctx: &InvocationContext,
    input: &str,
    state: &GeneralTurnLoopState,
) -> bool {
    can_use_local_directory_probe(ctx)
        && is_local_path_analysis_request(input)
        && !has_file_probe_tool_call(state)
}

pub(crate) fn should_require_structure_probe(
    _ctx: &InvocationContext,
    input: &str,
    state: &GeneralTurnLoopState,
) -> bool {
    is_local_path_analysis_request(input)
        && has_file_probe_tool_call(state)
        && !has_structure_probe_tool_call(state)
}

fn is_read_only_tool_name(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "read"
            | "read_file"
            | "list_dir"
            | "grep_files"
            | "memory_search"
            | "memory_get"
            | "web_search"
            | "web_fetch"
            | "http_request"
            | "list_mcp_resources"
            | "list_mcp_resource_templates"
    )
}

pub(crate) fn is_read_only_tool_round(step: &GeneralTurnStepResult) -> bool {
    !step.tool_calls.is_empty()
        && step
            .tool_calls
            .iter()
            .all(|call| is_read_only_tool_name(&call.name))
}

pub(crate) fn select_flow_profile(
    settings: &KernelExecutionSettings,
    tool_registry: &dyn ToolDefinitionProvider,
) -> FlowProfile {
    match settings.planner_kind {
        crate::PlannerKind::BuiltIn => {
            if tool_registry.get_tool_definitions().is_empty() {
                FlowProfile::DirectAnswer
            } else {
                FlowProfile::ModelThinking
            }
        }
        crate::PlannerKind::PlanReAct => FlowProfile::ReactiveLoop,
    }
}

impl FlowFactory {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        agent_name: impl Into<String>,
        agent_description: impl Into<String>,
        llm_provider: Arc<dyn LlmProvider>,
        tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
        tool_executor_port: Arc<dyn ToolExecutorPort>,
        memory_store: Arc<MemoryStore>,
        kernel_config: AgentKernelConfig,
        planner: Arc<dyn Planner>,
    ) -> Box<dyn BaseLlmFlow> {
        let settings = KernelExecutionSettings::try_from(&kernel_config).unwrap_or_default();
        let single = SingleFlow::new(
            agent_name,
            agent_description,
            llm_provider,
            tool_definition_provider,
            tool_executor_port,
            memory_store,
            kernel_config,
            planner,
        );

        match settings.flow_kind {
            FlowKind::Single => Box::new(single),
            FlowKind::Auto => Box::new(AutoFlow::new(single)),
        }
    }
}
