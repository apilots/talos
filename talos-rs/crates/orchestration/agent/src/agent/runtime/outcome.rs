use super::super::kernel;
use super::super::request::AgentKernelParams;
use super::KernelTimelineState;
use crate::{AgentError, AgentRunResult};
use talos_agent_kernel::{
    GeneralTurnOutcome, GeneralTurnResult, StopReason as KernelStopReason, TimelineBuilder,
};
use talos_error::{ErrorCode, TalosError};
use talos_types::agent_dispatch::AgentDispatchResumeContext;
use talos_types::talos::{
    RunItem, StopReason as TalosStopReason, TokenUsage, TurnExecutionMetrics,
};

pub(super) fn map_run_task_spawn_error(error: talos_run_control::SpawnError) -> AgentError {
    match error {
        talos_run_control::SpawnError::Closed => {
            TalosError::public(ErrorCode::Cancelled, "Run stopped accepting Runtime tasks").into()
        }
        talos_run_control::SpawnError::CapacityExceeded => {
            AgentError::runtime_error("Run task capacity exceeded".to_string())
        }
    }
}

pub(super) fn execution_from_general_turn_result(
    result: &GeneralTurnResult,
) -> (String, Vec<RunItem>, TalosStopReason, TokenUsage) {
    match &result.outcome {
        GeneralTurnOutcome::Completed {
            output_text,
            output_items,
            stop_reason,
            usage,
            ..
        } => (
            output_text.clone(),
            output_items.clone(),
            kernel::map_stop_reason(*stop_reason),
            usage.clone(),
        ),
        GeneralTurnOutcome::Waiting {
            output_items,
            stop_reason,
            usage,
            ..
        } => (
            String::new(),
            output_items.clone(),
            kernel::map_stop_reason(*stop_reason),
            usage.clone(),
        ),
    }
}

pub(super) fn agent_run_result_from_general_turn_result(
    result: &GeneralTurnResult,
    session_id: String,
) -> AgentRunResult {
    let (output_text, output_items, stop_reason, usage) =
        execution_from_general_turn_result(result);
    let reasoning = AgentKernelParams::collect_reasoning(&output_items);
    AgentRunResult {
        output_text,
        output_items,
        reasoning,
        stop_reason,
        usage,
        task_count: 1,
        session_id,
        generated_session_title: None,
    }
}

pub(super) fn outcome_stop_reason(outcome: &GeneralTurnOutcome) -> KernelStopReason {
    match outcome {
        GeneralTurnOutcome::Completed { stop_reason, .. }
        | GeneralTurnOutcome::Waiting { stop_reason, .. } => *stop_reason,
    }
}

pub(super) fn outcome_usage(outcome: &GeneralTurnOutcome) -> TokenUsage {
    match outcome {
        GeneralTurnOutcome::Completed { usage, .. } | GeneralTurnOutcome::Waiting { usage, .. } => {
            usage.clone()
        }
    }
}

pub(super) fn outcome_metrics(outcome: &GeneralTurnOutcome) -> TurnExecutionMetrics {
    match outcome {
        GeneralTurnOutcome::Completed { metrics, .. }
        | GeneralTurnOutcome::Waiting { metrics, .. } => metrics.clone(),
    }
}

pub(super) fn seed_resume_context(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    resume_context: Option<AgentDispatchResumeContext>,
) {
    let Some(context) = resume_context else {
        return;
    };
    for tool_call in context.tool_calls {
        timeline.track_existing_tool_call(
            tool_call.item_id.clone(),
            tool_call.call_id.clone(),
            tool_call.tool_name,
            String::new(),
        );
        state
            .tool_item_by_call_id
            .insert(tool_call.call_id.clone(), tool_call.item_id.clone());
        state
            .active_tool_items
            .insert(tool_call.call_id, tool_call.item_id);
    }
}
