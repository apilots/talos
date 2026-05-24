use super::*;

pub(crate) fn to_api_stop_reason(reason: StopReason) -> TalosApiStopReason {
    match reason {
        StopReason::EndTurn => TalosApiStopReason::EndTurn,
        StopReason::ModelOutputLimit => TalosApiStopReason::ModelOutputLimit,
        StopReason::ToolRoundLimit => TalosApiStopReason::ToolRoundLimit,
        StopReason::WaitingDynamicInput => TalosApiStopReason::WaitingDynamicInput,
        StopReason::TransferUnresolved => TalosApiStopReason::TransferUnresolved,
        StopReason::BeforeModelCallbackStopped => TalosApiStopReason::BeforeModelCallbackStopped,
        StopReason::AfterModelCallbackStopped => TalosApiStopReason::AfterModelCallbackStopped,
    }
}

pub(crate) fn build_turn_execution_metrics(
    state: &GeneralTurnLoopState,
    stop_reason: StopReason,
) -> TurnExecutionMetrics {
    let tool_calls = state.tool_call_count as u32;
    let tool_errors = state.tool_error_count as u32;
    TurnExecutionMetrics {
        model_rounds: state.round.saturating_add(1),
        tool_calls,
        tool_successes: tool_calls.saturating_sub(tool_errors),
        tool_errors,
        stop_reason: to_api_stop_reason(stop_reason),
        usage: state.usage.clone(),
        duration_ms: None,
    }
}

pub fn finalize_general_turn_artifacts(
    state: GeneralTurnLoopState,
    final_text: String,
    stop_reason: StopReason,
) -> ExecutedTurnArtifacts {
    let metrics = build_turn_execution_metrics(&state, stop_reason);
    let mut output_items = state.output_items;
    let reasoning_log = state.reasoning_log;
    let mut final_output = final_text;

    if matches!(stop_reason, StopReason::WaitingDynamicInput) {
        final_output.clear();
    } else if final_output.trim().is_empty() {
        final_output = if state.tool_call_count > 0 {
            "Tool execution completed.".to_string()
        } else {
            "No response generated.".to_string()
        };
    }

    if !matches!(stop_reason, StopReason::WaitingDynamicInput) {
        output_items.push(TalosOutputItem::OutputText {
            text: final_output.clone(),
        });
    }
    ExecutedTurnArtifacts {
        output_text: final_output,
        output_items,
        stop_reason,
        usage: state.usage,
        metrics,
        reasoning_log,
        tool_call_log: state.tool_call_log,
        tool_result_log: state.tool_result_log,
        pending_dynamic_requests: state.pending_dynamic_requests,
        final_answer_streamed: state.final_answer_streamed,
        execution_summary_streamed: state.execution_summary_streamed,
    }
}

pub async fn emit_general_turn_finalization_stream(
    prepared: &PreparedGeneralTurn,
    executed: &ExecutedTurnArtifacts,
) {
    if matches!(executed.stop_reason, StopReason::WaitingDynamicInput) {
        return;
    }

    if !executed.final_answer_streamed && !executed.output_text.trim().is_empty() {
        emit_assistant_text_stream_item(
            prepared.stream_tx.as_ref(),
            KernelStreamItemKey {
                round: None,
                local_id: "finalization_output".to_string(),
            },
            KernelAssistantTextPhase::FinalAnswer,
            None,
            &executed.output_text,
        )
        .await;
    }
}
