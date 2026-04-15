use super::*;

pub fn build_general_turn_step_request(
    prepared: &PreparedGeneralTurn,
    state: &GeneralTurnLoopState,
) -> GeneralTurnStepRequest {
    GeneralTurnStepRequest {
        round: state.round,
        model: prepared.selected_model.clone(),
        messages: state.messages.clone(),
        tool_definitions: prepared.tool_definitions.clone(),
        parallel_tool_calls: true,
        native_thinking: None,
        stream_tx: prepared.stream_tx.clone(),
    }
}
