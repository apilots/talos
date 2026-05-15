use crate::execution::ContentsMode;
use crate::general_turn::{
    apply_general_turn_step, apply_general_turn_tool_batch, build_general_turn_step_request,
    collect_general_turn_tool_batch, emit_general_turn_finalization_stream,
    execute_general_turn_step, execute_general_turn_tool_batch, finalize_general_turn_artifacts,
    persist_general_turn_artifacts, prepare_general_turn, GeneralTurnLoopState,
    GeneralTurnStepDisposition, GeneralTurnStepResult, GeneralTurnToolBatch, PlanReActLoopState,
};
use crate::{
    execution::DynamicToolRequestPayload, CallbackControl, CallbackRegistry,
    ConfirmationRequestPayload, ConfirmationRequestStreamData, EventAuthor, EventPayload,
    ExecutionBranch, ExecutionEvent, ExecutionEventKind, FlowKind, FlowProfile, InvocationContext,
    InvocationPluginManager, KernelExecutionSettings, KernelStreamEvent, KernelStreamEventData,
    KernelStreamEventType, KernelThinkingSource, KernelThinkingStage, KernelTranscriptEvent,
    KernelTranscriptEventKind, KernelTranscriptPayload, KernelTranscriptService, KernelWarning,
    LlmRequestEnvelope, LlmResponseEnvelope, MemoryBackedKernelTranscriptService, Planner,
    RequestProcessorPipeline, ResponseProcessorPipeline, StopReason, ThinkingDeltaStreamData,
    ToolCallbackPayload, ToolExecutor, ToolExecutorFactory,
};
use async_trait::async_trait;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_llm_provider::LlmProvider;
use talos_memory::MemoryStore;
use talos_tools::{ToolDefinitionProvider, ToolExecutorPort};
use talos_types::talos::{TalosOutputItem, TalosUsage};
use tokio::sync::mpsc;

mod auto;
mod callbacks;
#[cfg(test)]
mod convergence_tests;
mod factory;
mod single;
#[cfg(test)]
mod tests;
mod types;

pub use auto::AutoFlow;
pub use factory::FlowFactory;
pub use single::SingleFlow;
pub use types::{BaseLlmFlow, FlowExecution, FlowRunSummary};

pub(crate) use callbacks::{
    build_flow_step_model_request, emit_framework_thinking, has_final_response,
    stage_kernel_transcript_event, tool_names_for_thinking,
};
#[cfg(test)]
pub(crate) use factory::is_local_path_analysis_request;
pub(crate) use factory::{is_read_only_tool_round, select_flow_profile, should_replan};
