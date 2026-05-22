mod callbacks;
mod contents_builder;
mod execution;
mod general_turn;
mod planner;
mod plugin;
mod processor;
mod session;
mod timeline;
mod tool_execution;
mod traits;
mod types;
mod warnings;

use std::sync::Arc;

pub use callbacks::{
    AfterModelCallback, AfterToolCallback, BeforeModelCallback, BeforeToolCallback,
    CallbackControl, CallbackKind, CallbackRegistry, ToolCallbackPayload,
};
pub use contents_builder::ContentsBuilder;
pub use execution::ReplayToolInvocation;
pub use execution::{
    AuthRequestPayload, CompactionSummaryPayload, EventActions, EventAuthor, EventPayload,
    EventSink, ExecutionBranch, ExecutionEvent, ExecutionEventBuffer, ExecutionEventHandler,
    ExecutionEventKind, LlmRequestEnvelope, LlmResponseEnvelope, PlanningRequestEffect,
    PlanningResponseEffect, PlanningResponseSegment, PlanningResponseSegmentKind,
    RequestCacheContext, RequestInteractionContext, RequestOutputContract, ResponseDisposition,
    StateDelta, StateNamespace, VecEventSink,
};
pub use general_turn::{
    apply_general_turn_step, apply_general_turn_tool_batch, build_general_turn_step_request,
    emit_general_turn_finalization_stream, execute_general_turn_loop,
    execute_general_turn_loop_with_policy, execute_general_turn_loop_with_resume,
    execute_general_turn_step, execute_general_turn_tool_batch, finalize_general_turn_artifacts,
    flush_session_memory, flush_session_memory_with_report, persist_general_turn_artifacts,
    prepare_general_turn, run_general_turn, run_general_turn_with_resume, BuiltInGeneralTurnPolicy,
    DynamicToolResultResume, ExecutedTurnArtifacts, GeneralTurnLoopState, GeneralTurnOutcome,
    GeneralTurnPolicy, GeneralTurnPolicyStop, GeneralTurnResult, GeneralTurnResume,
    GeneralTurnStepDisposition, GeneralTurnStepRequest, GeneralTurnStepResult,
    GeneralTurnToolBatch, MemoryFlushPolicy, MemoryFlushUsageReport, ModelFinishReason,
    PendingGeneralTurnAction, PlanReActLoopState, PreparedGeneralTurn, ResumeToolCall,
    StreamingToolExecutionConfig,
};
pub use planner::{BuiltInPlanner, PlanReActPlanner};
pub use plugin::{InvocationPluginManager, PluginRegistry};
pub use processor::{
    AgentTransferRequestProcessor, BasicRequestProcessor, ContentsRequestProcessor,
    EventProjectionProcessor, FinalAnswerExtractionProcessor, InstructionsRequestProcessor,
    PlanningRequestProcessor, PlanningResponseProcessor, RequestProcessor, RequestProcessorKind,
    RequestProcessorPipeline, ResponseProcessor, ResponseProcessorKind, ResponseProcessorPipeline,
    StateDeltaResponseProcessor, ToolCallExtractionProcessor, ToolContextRequestProcessor,
};
pub use session::{
    InvocationSession, KernelTranscriptEvent, KernelTranscriptEventKind, KernelTranscriptFilter,
    KernelTranscriptPayload, KernelTranscriptService, MemoryBackedKernelTranscriptService,
    SessionState,
};
pub use timeline::TimelineBuilder;
pub use tool_execution::{
    ParallelToolExecutor, SequentialToolExecutor, StreamingToolExecutionOutput,
    StreamingToolScheduler, ToolExecutionContext, ToolExecutionInput, ToolExecutionOutput,
    ToolExecutor, ToolExecutorFactory,
};
pub use traits::{Planner, Plugin};
pub use types::{
    AgentRunResult, AssistantTextCompletedStreamData, AssistantTextDeltaStreamData,
    AssistantTextStartedStreamData, ContextStateBags, DynamicToolRequestStreamData,
    ExecutionSummaryStreamData, FinalResponseEventData, FlowProfile, InvocationContext,
    InvocationExecutionState, InvocationLimits, IterationEventData, KernelAssistantTextPhase,
    KernelEvent, KernelEventData, KernelEventLabel, KernelEventType, KernelExecutionSettings,
    KernelModelCallMode, KernelStreamEvent, KernelStreamEventData, KernelStreamEventType,
    KernelStreamItemKey, KernelThinkingSource, KernelThinkingStage, PlannerKind,
    PromptWarningEventData, StopReason, TaskCompletedEventData, TaskKind, TaskResult, TaskStatus,
    ThinkingDeltaStreamData, ToolArgsDeltaStreamData, ToolCallStartStreamData,
    ToolExecutingStreamData, ToolExecutionMode, ToolProgressDeltaStreamData, ToolProgressKind,
    ToolQueuedStreamData, ToolResultStreamData, ToolStreamKey, TransferRequestedEventData,
    TurnPhase, UserInputEventData,
};
pub use warnings::{
    KernelWarning, PromptBootstrapDropReason, PromptBootstrapFileDroppedWarning,
    PromptBootstrapFileMissingWarning, PromptBootstrapFileTruncatedWarning,
    PromptDynamicSectionDroppedWarning,
};

pub fn default_plugin_manager() -> Arc<InvocationPluginManager> {
    Arc::new(InvocationPluginManager::new())
}
