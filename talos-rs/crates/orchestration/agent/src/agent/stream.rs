use super::AgentImpl;
use crate::service::{
    OrchestratorEvent, OrchestratorMessageExecutionSummaryEvent,
    OrchestratorMessageOutputDeltaEvent, OrchestratorMessageReasoningDeltaEvent,
    OrchestratorMessageToolCallArgsDeltaEvent, OrchestratorMessageToolCallCompletedEvent,
    OrchestratorMessageToolCallExecutingEvent, OrchestratorMessageToolCallPendingApprovalEvent,
    OrchestratorMessageToolCallProgressDeltaEvent, OrchestratorMessageToolCallQueuedEvent,
    OrchestratorMessageToolCallStartedEvent, OrchestratorToolProgressKind,
    OrchestratorToolStreamKey,
};
use serde_json::Value;
use talos_agent_kernel::{
    ConfirmationRequestStreamData, KernelStreamEvent, KernelStreamEventData, KernelStreamEventType,
    KernelThinkingSource, KernelThinkingStage, ToolProgressKind,
};
use talos_types::talos::{TalosContentItem, TalosThinkingSource, TalosThinkingStage};

impl AgentImpl {
    pub(crate) fn map_kernel_stream_event(
        event: KernelStreamEvent,
        request_id: &str,
        item_index: &mut u64,
    ) -> Option<OrchestratorEvent> {
        let current_index = *item_index;
        let iteration = current_index.saturating_add(1);
        let step_id = format!("step-{}", iteration);
        let mapped = match (event.event_type, event.data) {
            (KernelStreamEventType::ThinkingDelta, KernelStreamEventData::ThinkingDelta(data)) => {
                Some(OrchestratorEvent::MessageReasoningDelta(
                    OrchestratorMessageReasoningDeltaEvent {
                        request_id: request_id.to_string(),
                        delta: data.delta,
                        source: map_thinking_source(data.source),
                        stage: map_thinking_stage(data.stage),
                        item_index: current_index,
                        iteration,
                        step_id,
                    },
                ))
            }
            (
                KernelStreamEventType::ExecutionSummary,
                KernelStreamEventData::ExecutionSummary(data),
            ) => Some(OrchestratorEvent::MessageExecutionSummary(
                OrchestratorMessageExecutionSummaryEvent {
                    request_id: request_id.to_string(),
                    text: data.text,
                    item_index: current_index,
                    iteration,
                    step_id,
                },
            )),
            (KernelStreamEventType::OutputDelta, KernelStreamEventData::OutputDelta(data)) => Some(
                OrchestratorEvent::MessageOutputDelta(OrchestratorMessageOutputDeltaEvent {
                    request_id: request_id.to_string(),
                    delta: data.delta,
                    item_index: current_index,
                    iteration,
                    step_id,
                }),
            ),
            (KernelStreamEventType::ToolCallStart, KernelStreamEventData::ToolCallStart(data)) => {
                Some(OrchestratorEvent::MessageToolCallStarted(
                    OrchestratorMessageToolCallStartedEvent {
                        request_id: request_id.to_string(),
                        call_id: data.call_id.unwrap_or_else(|| "unknown".to_string()),
                        name: data.tool_name.unwrap_or_else(|| "unknown".to_string()),
                        arguments: data.arguments,
                        item_index: current_index,
                        iteration,
                        step_id,
                        timing_ms: None,
                    },
                ))
            }
            (KernelStreamEventType::ToolArgsDelta, KernelStreamEventData::ToolArgsDelta(data)) => {
                Some(OrchestratorEvent::MessageToolCallArgsDelta(
                    OrchestratorMessageToolCallArgsDeltaEvent {
                        request_id: request_id.to_string(),
                        stream_key: map_tool_stream_key(data.stream_key),
                        call_id: data.call_id,
                        tool_name: data.tool_name,
                        delta: data.delta,
                        item_index: current_index,
                        iteration,
                        step_id,
                    },
                ))
            }
            (KernelStreamEventType::ToolQueued, KernelStreamEventData::ToolQueued(data)) => Some(
                OrchestratorEvent::MessageToolCallQueued(OrchestratorMessageToolCallQueuedEvent {
                    request_id: request_id.to_string(),
                    stream_key: map_tool_stream_key(data.stream_key),
                    call_id: data.call_id,
                    tool_name: data.tool_name,
                    item_index: current_index,
                    iteration,
                    step_id,
                }),
            ),
            (KernelStreamEventType::ToolExecuting, KernelStreamEventData::ToolExecuting(data)) => {
                Some(OrchestratorEvent::MessageToolCallExecuting(
                    OrchestratorMessageToolCallExecutingEvent {
                        request_id: request_id.to_string(),
                        stream_key: map_tool_stream_key(data.stream_key),
                        call_id: data.call_id,
                        tool_name: data.tool_name,
                        item_index: current_index,
                        iteration,
                        step_id,
                    },
                ))
            }
            (
                KernelStreamEventType::ToolProgressDelta,
                KernelStreamEventData::ToolProgressDelta(data),
            ) => Some(OrchestratorEvent::MessageToolCallProgressDelta(
                OrchestratorMessageToolCallProgressDeltaEvent {
                    request_id: request_id.to_string(),
                    stream_key: map_tool_stream_key(data.stream_key),
                    call_id: data.call_id,
                    tool_name: data.tool_name,
                    message: data.message,
                    progress_kind: map_tool_progress_kind(data.progress_kind),
                    item_index: current_index,
                    iteration,
                    step_id,
                },
            )),
            (KernelStreamEventType::ToolResult, KernelStreamEventData::ToolResult(data)) => {
                Some(OrchestratorEvent::MessageToolCallCompleted(
                    OrchestratorMessageToolCallCompletedEvent {
                        request_id: request_id.to_string(),
                        call_id: data.call_id,
                        tool: data.tool_name,
                        content: if data.content.is_empty() {
                            vec![TalosContentItem::Json {
                                value: Value::Array(Vec::new()),
                            }]
                        } else {
                            data.content
                        },
                        is_error: data.is_error,
                        item_index: current_index,
                        iteration,
                        step_id,
                        timing_ms: None,
                    },
                ))
            }
            (KernelStreamEventType::PromptWarning, KernelStreamEventData::PromptWarning(data)) => {
                Some(OrchestratorEvent::MessageExecutionSummary(
                    OrchestratorMessageExecutionSummaryEvent {
                        request_id: request_id.to_string(),
                        text: format!("Prompt warning: {:?}", data.warning),
                        item_index: current_index,
                        iteration,
                        step_id,
                    },
                ))
            }
            (
                KernelStreamEventType::ConfirmationRequest,
                KernelStreamEventData::ConfirmationRequest(ConfirmationRequestStreamData {
                    call_id,
                    tool_name,
                    prompt,
                    arguments,
                }),
            ) => Some(OrchestratorEvent::MessageToolCallPendingApproval(
                OrchestratorMessageToolCallPendingApprovalEvent {
                    request_id: request_id.to_string(),
                    call_id,
                    tool_name,
                    prompt,
                    arguments,
                    step_id,
                },
            )),
            _ => None,
        };
        if mapped.is_some() {
            *item_index = (*item_index).saturating_add(1);
        }
        mapped
    }
}

fn map_thinking_source(source: KernelThinkingSource) -> TalosThinkingSource {
    match source {
        KernelThinkingSource::Provider => TalosThinkingSource::Provider,
        KernelThinkingSource::Framework => TalosThinkingSource::Framework,
    }
}

fn map_thinking_stage(stage: KernelThinkingStage) -> TalosThinkingStage {
    match stage {
        KernelThinkingStage::ModelReasoning => TalosThinkingStage::ModelReasoning,
        KernelThinkingStage::FrameworkPlan => TalosThinkingStage::FrameworkPlan,
        KernelThinkingStage::FrameworkAct => TalosThinkingStage::FrameworkAct,
        KernelThinkingStage::FrameworkObserve => TalosThinkingStage::FrameworkObserve,
        KernelThinkingStage::FrameworkReflect => TalosThinkingStage::FrameworkReflect,
    }
}

fn map_tool_stream_key(stream_key: talos_agent_kernel::ToolStreamKey) -> OrchestratorToolStreamKey {
    OrchestratorToolStreamKey {
        round: stream_key.round,
        index: stream_key.index,
    }
}

fn map_tool_progress_kind(kind: ToolProgressKind) -> OrchestratorToolProgressKind {
    match kind {
        ToolProgressKind::Status => OrchestratorToolProgressKind::Status,
        ToolProgressKind::Stdout => OrchestratorToolProgressKind::Stdout,
        ToolProgressKind::Stderr => OrchestratorToolProgressKind::Stderr,
    }
}
