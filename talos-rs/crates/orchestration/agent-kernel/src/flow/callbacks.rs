use super::*;

pub(crate) fn build_flow_step_model_request(
    selected_model_info: Option<talos_types::ModelInfo>,
    planning_instruction: Option<String>,
    native_thinking: Option<talos_llm_provider::NativeThinkingConfig>,
    branch: ExecutionBranch,
    prepared: &crate::general_turn::PreparedGeneralTurn,
    state: &GeneralTurnLoopState,
    settings: &KernelExecutionSettings,
) -> LlmRequestEnvelope {
    LlmRequestEnvelope {
        selected_model_info,
        prompt_prefix_messages: prepared.prompt_prefix_messages.clone(),
        transient_messages: state.messages.clone(),
        planning_instruction,
        tool_definitions: prepared.tool_definitions.clone(),
        parallel_tool_calls: settings.tool_execution_mode == crate::ToolExecutionMode::Parallel,
        contents_mode: ContentsMode::FullSession,
        branch,
        native_thinking,
        ..LlmRequestEnvelope::default()
    }
}

pub(crate) fn stage_session_event(ctx: &mut InvocationContext, event: SessionEvent) {
    for delta in &event.actions.state_delta {
        ctx.session.state.apply_delta(delta);
    }
    ctx.session.events.push(event);
}

pub(crate) async fn emit_framework_thinking(
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
    stage: KernelThinkingStage,
    round: u32,
    delta: impl Into<String>,
) {
    let Some(tx) = stream_tx else {
        return;
    };
    let _ = tx
        .send(KernelStreamEvent::new(
            KernelStreamEventType::ThinkingDelta,
            KernelStreamEventData::ThinkingDelta(ThinkingDeltaStreamData {
                delta: delta.into(),
                source: KernelThinkingSource::Framework,
                stage,
                round: Some(round),
            }),
        ))
        .await;
}

pub(crate) fn tool_names_for_thinking(output_items: &[TalosOutputItem]) -> Vec<String> {
    output_items
        .iter()
        .filter_map(|item| match item {
            TalosOutputItem::ToolCall { name, .. } => Some(name.clone()),
            TalosOutputItem::ToolResult { tool_name, .. } => Some(tool_name.clone()),
            _ => None,
        })
        .collect()
}

pub(crate) fn has_final_response(events: &[ExecutionEvent], branch: ExecutionBranch) -> bool {
    events
        .iter()
        .rev()
        .any(|event| event.branch == branch && event.is_final_response())
}
