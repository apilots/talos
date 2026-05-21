use super::*;

pub(crate) fn to_api_stop_reason(reason: StopReason) -> TalosApiStopReason {
    match reason {
        StopReason::EndTurn => TalosApiStopReason::EndTurn,
        StopReason::ModelOutputLimit => TalosApiStopReason::ModelOutputLimit,
        StopReason::ToolRoundLimit => TalosApiStopReason::ToolRoundLimit,
        StopReason::TransferUnresolved => TalosApiStopReason::TransferUnresolved,
        StopReason::BeforeModelCallbackStopped => TalosApiStopReason::BeforeModelCallbackStopped,
        StopReason::AfterModelCallbackStopped => TalosApiStopReason::AfterModelCallbackStopped,
    }
}

pub(crate) fn build_execution_summary(tool_call_count: usize, tool_error_count: usize) -> String {
    if tool_call_count == 0 {
        "Kernel summary: answered directly without invoking tools.".to_string()
    } else if tool_error_count == 0 {
        format!(
            "Kernel summary: executed {} tool call(s) successfully before final response.",
            tool_call_count
        )
    } else {
        format!(
            "Kernel summary: executed {} tool call(s), {} call(s) returned errors; final response generated with available results.",
            tool_call_count, tool_error_count
        )
    }
}

pub fn finalize_general_turn_artifacts(
    state: GeneralTurnLoopState,
    final_text: String,
    stop_reason: StopReason,
) -> ExecutedTurnArtifacts {
    let mut output_items = state.output_items;
    let mut reasoning_log = state.reasoning_log;
    let mut final_output = final_text;

    if final_output.trim().is_empty() {
        final_output = if state.tool_call_count > 0 {
            "Tool execution completed.".to_string()
        } else {
            "No response generated.".to_string()
        };
    }

    let execution_summary = build_execution_summary(state.tool_call_count, state.tool_error_count);
    output_items.push(TalosOutputItem::ExecutionSummary {
        text: execution_summary.clone(),
    });
    output_items.push(TalosOutputItem::OutputText {
        text: final_output.clone(),
    });
    reasoning_log.push(execution_summary);

    ExecutedTurnArtifacts {
        output_text: final_output,
        output_items,
        stop_reason,
        usage: state.usage,
        reasoning_log,
        tool_call_log: state.tool_call_log,
        tool_result_log: state.tool_result_log,
        final_answer_streamed: state.final_answer_streamed,
        execution_summary_streamed: state.execution_summary_streamed,
    }
}

pub async fn emit_general_turn_finalization_stream(
    prepared: &PreparedGeneralTurn,
    executed: &ExecutedTurnArtifacts,
) {
    if !executed.execution_summary_streamed {
        if let Some(execution_summary) = executed.reasoning_log.last() {
            emit_stream_event(
                prepared.stream_tx.as_ref(),
                KernelStreamEventType::ExecutionSummary,
                KernelStreamEventData::ExecutionSummary(ExecutionSummaryStreamData {
                    text: execution_summary.clone(),
                    round: None,
                }),
            )
            .await;
        }
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
