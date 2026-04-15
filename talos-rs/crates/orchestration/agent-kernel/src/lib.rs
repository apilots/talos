mod callbacks;
mod composite;
mod contents_builder;
mod execution;
mod flow;
mod general_turn;
mod llm_agent;
mod planner;
mod planner_selector;
mod plugin;
mod processor;
mod runner;
mod runtime;
mod session;
mod tool_execution;
mod traits;
mod types;
mod warnings;

use std::sync::Arc;

pub use callbacks::{
    AfterModelCallback, AfterToolCallback, BeforeModelCallback, BeforeToolCallback,
    CallbackControl, CallbackKind, CallbackRegistry, ToolCallbackPayload,
};
pub use composite::{
    LoopKernelAgent, ParallelKernelAgent, RoutingKernelAgent, SequentialKernelAgent,
};
pub use contents_builder::ContentsBuilder;
pub use execution::ReplayToolInvocation;
pub use execution::{
    AuthRequestPayload, CompactionSummaryPayload, ConfirmationRequestPayload, EventActions,
    EventAuthor, EventPayload, EventSink, ExecutionBranch, ExecutionEvent, ExecutionEventBuffer,
    ExecutionEventHandler, ExecutionEventKind, LlmRequestEnvelope, LlmResponseEnvelope,
    PlanningRequestEffect, PlanningResponseEffect, PlanningResponseSegment,
    PlanningResponseSegmentKind, RequestCacheContext, RequestInteractionContext,
    RequestOutputContract, ResponseDisposition, StateDelta, StateNamespace, VecEventSink,
};
pub use flow::{AutoFlow, BaseLlmFlow, FlowExecution, FlowFactory, FlowRunSummary, SingleFlow};
pub use general_turn::{
    apply_general_turn_step, apply_general_turn_tool_batch, build_general_turn_step_request,
    emit_general_turn_finalization_stream, execute_general_turn_loop, execute_general_turn_step,
    execute_general_turn_tool_batch, finalize_general_turn_artifacts, flush_session_memory,
    persist_general_turn_artifacts, prepare_general_turn, run_general_turn, ExecutedTurnArtifacts,
    GeneralTurnLoopState, GeneralTurnResult, GeneralTurnStepDisposition, GeneralTurnStepRequest,
    GeneralTurnStepResult, GeneralTurnToolBatch, ModelFinishReason, PlanReActLoopState,
    PreparedGeneralTurn, StreamingToolExecutionConfig,
};
pub use llm_agent::LlmKernelAgent;
pub use planner::{BuiltInPlanner, PlanReActPlanner};
pub use planner_selector::{
    LlmCallError, LlmCallStatus, PlannerSelectionRequest, PlannerSelectionResponse, PlannerSelector,
};
pub use plugin::{InvocationPluginManager, PluginRegistry};
pub use processor::{
    AgentTransferRequestProcessor, BasicRequestProcessor, ContentsRequestProcessor,
    EventProjectionProcessor, FinalAnswerExtractionProcessor, InstructionsRequestProcessor,
    PlanningRequestProcessor, PlanningResponseProcessor, RequestProcessor, RequestProcessorKind,
    RequestProcessorPipeline, ResponseProcessor, ResponseProcessorKind, ResponseProcessorPipeline,
    StateDeltaResponseProcessor, ToolCallExtractionProcessor, ToolContextRequestProcessor,
};
pub use runner::DefaultRunner;
pub use runtime::{KernelBuilder, KernelOutput, KernelRuntime};
pub use session::{
    InvocationSession, MemoryBackedSessionService, SessionApprovalResponsePayload,
    SessionConfirmationPayload, SessionEvent, SessionEventFilter, SessionEventKind,
    SessionEventPayload, SessionService, SessionState,
};
pub use tool_execution::{
    ParallelToolExecutor, SequentialToolExecutor, StreamingToolExecutionOutput,
    StreamingToolScheduler, ToolExecutionContext, ToolExecutionInput, ToolExecutionOutput,
    ToolExecutor, ToolExecutorFactory,
};
pub use traits::{Agent, BaseKernelAgent, Planner, Plugin, Runner};
pub use types::{
    AgentRunResult, ConfirmationRequestStreamData, ContextStateBags, ExecutionPath,
    ExecutionSummaryStreamData, FinalResponseEventData, FlowKind, FlowProfile, InvocationContext,
    InvocationExecutionState, InvocationLimits, IterationEventData, KernelEvent, KernelEventData,
    KernelEventLabel, KernelEventType, KernelExecutionKind, KernelExecutionSettings,
    KernelOutputDeltaSource, KernelStreamEvent, KernelStreamEventData, KernelStreamEventType,
    KernelThinkingSource, KernelThinkingStage, OutputDeltaStreamData, PlannerKind,
    PromptWarningEventData, RunRequest, RunResponse, StopReason, TaskCompletedEventData, TaskKind,
    TaskResult, TaskStatus, ThinkingDeltaStreamData, ThinkingMode, ToolArgsDeltaStreamData,
    ToolCallStartStreamData, ToolExecutingStreamData, ToolExecutionMode,
    ToolProgressDeltaStreamData, ToolProgressKind, ToolQueuedStreamData, ToolResultStreamData,
    ToolStreamKey, TransferRequestedEventData, TurnPhase, UserInputEventData,
};
pub use warnings::{
    KernelWarning, PromptBootstrapDropReason, PromptBootstrapFileDroppedWarning,
    PromptBootstrapFileMissingWarning, PromptBootstrapFileTruncatedWarning,
    PromptDynamicSectionDroppedWarning, PromptDynamicSectionTruncatedWarning,
};

pub fn default_plugin_manager() -> Arc<InvocationPluginManager> {
    Arc::new(InvocationPluginManager::new())
}
