mod contents_builder;
mod execution;
mod general_turn;
mod input;
mod planner;
mod processor;
mod session;
mod session_context;
mod timeline;
mod tool_execution;
mod traits;
mod types;
mod warnings;

pub use contents_builder::ContentsBuilder;
pub use execution::ReplayToolInvocation;
pub use execution::{
    AuthRequestPayload, CompactionSummaryPayload, EventActions, EventAuthor, EventCompaction,
    EventPayload, EventSink, ExecutionBranch, ExecutionEvent, ExecutionEventBuffer,
    ExecutionEventHandler, ExecutionEventKind, LlmRequestEnvelope, LlmResponseEnvelope,
    PlanningRequestEffect, PlanningResponseEffect, PlanningResponseSegment,
    PlanningResponseSegmentKind, RequestCacheContext, RequestInteractionContext,
    RequestOutputContract, StateDelta, StateNamespace, VecEventSink,
};
pub use general_turn::{
    apply_general_turn_step, apply_general_turn_tool_batch, build_active_turn_input,
    build_general_turn_step_request, compact_session_memory, emit_general_turn_finalization_stream,
    execute_general_turn_loop, execute_general_turn_loop_with_policy,
    execute_general_turn_loop_with_resume, execute_general_turn_step,
    execute_general_turn_tool_batch, finalize_general_turn_artifacts, form_daily_session_memory,
    persist_general_turn_artifacts, prepare_general_turn, run_general_turn,
    run_general_turn_with_resume, BuiltInGeneralTurnPolicy, ExecutedTurnArtifacts,
    GeneralTurnLoopState, GeneralTurnOutcome, GeneralTurnPolicy, GeneralTurnPolicyStop,
    GeneralTurnResult, GeneralTurnResume, GeneralTurnSkillOptions, GeneralTurnStepDisposition,
    GeneralTurnStepRequest, GeneralTurnStepResult, GeneralTurnToolBatch, MemoryCompactionPolicy,
    MemoryFormationParams, MemoryFormationReport, ModelFinishReason, PendingGeneralTurnAction,
    PreparedGeneralTurn, ResumeToolCall, SessionCompactionState, StreamingToolExecutionConfig,
    UserActionResume, SESSION_COMPACTION_STATE_KEY,
};
pub use input::{
    ActiveTurnInput, ActiveTurnInputDelivery, AttachmentReader, RejectingAttachmentReader,
    ResolvedAttachment, TurnFileScope, UserInput,
};
pub use planner::BuiltInPlanner;
pub use processor::{
    AgentTransferRequestProcessor, BasicRequestProcessor, ContentsRequestProcessor,
    EventProjectionProcessor, InstructionsRequestProcessor, PlanningRequestProcessor,
    PlanningResponseProcessor, RequestProcessor, RequestProcessorKind, RequestProcessorPipeline,
    ResponseProcessor, ResponseProcessorKind, ResponseProcessorPipeline,
    StateDeltaResponseProcessor, ToolContextRequestProcessor,
};
pub use session::{
    InvocationSession, KernelTranscriptAuthPayload, KernelTranscriptCompactionPayload,
    KernelTranscriptEvent, KernelTranscriptEventKind, KernelTranscriptFilter,
    KernelTranscriptPayload, KernelTranscriptService, KernelTranscriptTransferPayload,
    KernelTranscriptUserActionRequestPayload, KernelTranscriptUserActionResponsePayload,
    SessionState,
};
pub use session_context::{
    build_session_prompt_context, SessionPromptContext, SessionPromptContextConfig,
    SessionPromptRecentWindow, SessionToolExchangeContext, SessionToolExchangeOutcome,
    SessionToolResultContext, TalosMessage, TalosMessageRole,
};
pub use timeline::TimelineBuilder;
pub use tool_execution::{
    ParallelToolExecutor, SequentialToolExecutor, StreamingToolExecutionOutput,
    StreamingToolScheduler, ToolExecutionContext, ToolExecutionInput, ToolExecutionOutput,
    ToolExecutor, ToolExecutorFactory,
};
pub use traits::Planner;
pub use types::{
    AssistantTextCommittedStreamData, AssistantTextCompletedStreamData,
    AssistantTextDeltaStreamData, AssistantTextPhase, AssistantTextStartedStreamData,
    ContextCompactionStreamData, ContextStateBags, ExecutionSummaryStreamData,
    FinalResponseEventData, FlowProfile, InvocationContext, InvocationExecutionState,
    InvocationLimits, IterationEventData, KernelEvent, KernelEventData, KernelEventLabel,
    KernelEventType, KernelExecutionSettings, KernelModelCallMode, KernelStreamEvent,
    KernelStreamEventData, KernelStreamEventType, KernelStreamItemKey, KernelThinkingSource,
    KernelThinkingStage, PlannerKind, PromptWarningEventData, StopReason, TaskCompletedEventData,
    TaskKind, TaskResult, TaskStatus, ThinkingDeltaStreamData, ToolArgsDeltaStreamData,
    ToolCallStartStreamData, ToolExecutingStreamData, ToolExecutionMode,
    ToolProgressDeltaStreamData, ToolProgressKind, ToolQueuedStreamData, ToolResultStreamData,
    ToolStreamKey, TransferRequestedEventData, TurnMetricsStreamData, TurnPhase,
    UserActionRequestStreamData, UserInputEventData,
};
pub use warnings::{
    KernelWarning, PromptBootstrapDropReason, PromptBootstrapFileDroppedWarning,
    PromptBootstrapFileMissingWarning, PromptBootstrapFileTruncatedWarning,
    PromptDynamicSectionDroppedWarning,
};

#[cfg(test)]
pub(crate) fn test_run_tasks() -> talos_run_control::RunTaskGroup {
    talos_run_control::ActiveRunControl::new(talos_run_control::RunIdentity::new(
        "test-session",
        "test-turn",
    ))
    .tasks()
}
