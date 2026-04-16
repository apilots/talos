use super::*;

pub fn build_general_turn_step_request(
    prepared: &PreparedGeneralTurn,
    state: &GeneralTurnLoopState,
    parallel_tool_calls: bool,
) -> GeneralTurnStepRequest {
    GeneralTurnStepRequest {
        round: state.round,
        model: prepared.selected_model.clone(),
        messages: state.messages.clone(),
        tool_definitions: prepared.tool_definitions.clone(),
        parallel_tool_calls,
        native_thinking: None,
        stream_tx: prepared.stream_tx.clone(),
        streaming_tool_execution: None,
    }
}
