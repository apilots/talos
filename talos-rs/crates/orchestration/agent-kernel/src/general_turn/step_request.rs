use super::*;

pub fn build_general_turn_step_request(
    prepared: &PreparedGeneralTurn,
    state: &GeneralTurnLoopState,
    tool_definitions: Vec<ToolDefinition>,
    parallel_tool_calls: bool,
) -> GeneralTurnStepRequest {
    let mut messages = prepared.messages.clone();
    messages.extend(state.messages.clone());

    GeneralTurnStepRequest {
        round: state.round,
        call_mode: KernelModelCallMode::WorkingLoop,
        model: prepared.selected_model.clone(),
        max_tokens: prepared.max_output_tokens,
        messages,
        tool_definitions,
        parallel_tool_calls,
        reasoning: prepared.reasoning,
        speed: prepared.speed,
        pre_final_execution_summary: None,
        stream_tx: prepared.stream_tx.clone(),
        streaming_tool_execution: None,
    }
}
