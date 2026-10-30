mod artifacts;
mod execution;
mod facade;
mod general_turn;
mod input;
mod session;
mod session_context;
mod tool_execution;
mod types;
mod warnings;

// Public facade consumed by the Kernel Provider adapter. Internal loop,
// processor, planner, and scheduler APIs stay crate-private.
pub use execution::{EventAuthor, ExecutionBranch, SessionStateUpdate};
pub use facade::{
    AgentKernel, AgentKernelDependencies, AgentKernelExecution, AgentKernelIo,
    AgentKernelPendingInput, AgentKernelRequest, AgentKernelResult,
};
pub use general_turn::{
    build_active_turn_input, compact_session_memory, form_dream_session_memory,
    AgentKernelContinuation, AgentKernelContinuationToolCall, AgentKernelUserActionContinuation,
    GeneralTurnSkillOptions, MemoryCompactionPolicy, MemoryFormationParams, SessionCompactionState,
};
pub(crate) use general_turn::{
    run_general_turn_with_resume, GeneralTurnOutcome, GeneralTurnResult,
};
pub use input::{ActiveTurnInput, ActiveTurnInputDelivery, ResolvedAttachment, UserInput};
pub use session::{
    InvocationSession, KernelTranscriptAuthPayload, KernelTranscriptCompactionPayload,
    KernelTranscriptEvent, KernelTranscriptPayload, KernelTranscriptService,
    KernelTranscriptTransferPayload, KernelTranscriptUserActionRequestPayload,
    KernelTranscriptUserActionResponsePayload, SessionState,
};
pub use session_context::{
    SessionPromptContext, SessionPromptContextConfig, SessionPromptContextFuture,
    SessionPromptContextSource, SessionPromptEntryHeader, SessionPromptMessage,
    SessionPromptMessageRole, SessionPromptRecentWindow, SessionPromptSourceTurn,
    SessionPromptSummary, SessionToolExchangeContext, SessionToolExchangeOutcome,
    SessionToolResultContext,
};
pub(crate) use tool_execution::{
    ToolExecutionInput, ToolExecutionOutput, ToolExecutor, ToolExecutorFactory,
};
pub(crate) use types::KernelRunItem;
pub use types::{
    AgentKernelEvent, AgentKernelEventPayload, AssistantTextCommittedStreamData,
    AssistantTextCompletedStreamData, AssistantTextDeltaStreamData, AssistantTextPhase,
    AssistantTextStartedStreamData, ContextCompactionStreamData, ExecutionSummaryStreamData,
    KernelStreamItemKey, PromptWarningEventData, StopReason, ThinkingDeltaStreamData,
    ToolArgsDeltaStreamData, ToolCallStartStreamData, ToolExecutingStreamData,
    ToolProgressDeltaStreamData, ToolProgressKind, ToolQueuedStreamData, ToolResultStreamData,
    ToolStreamKey, TurnMetricsStreamData, UserActionRequestStreamData,
};
pub(crate) use types::{
    InvocationContext, KernelExecutionSettings, KernelModelCallMode, KernelThinkingSource,
    KernelThinkingStage, ToolExecutionMode, TurnPhase,
};
pub use warnings::{
    KernelWarning, PromptBootstrapDropReason, PromptBootstrapFileDroppedWarning,
    PromptBootstrapFileMissingWarning, PromptBootstrapFileTruncatedWarning,
    PromptDynamicSectionDroppedWarning,
};

#[cfg(test)]
pub(crate) fn test_run_tasks() -> talos_run_control::RunTaskGroup {
    talos_run_control::ActiveRunControl::new(talos_types::AgentTurnRef::new(
        "test-session",
        "test-turn",
    ))
    .tasks()
}
pub use artifacts::{
    KernelArtifactHeader, KernelMemoryProgressStore, KernelSessionArtifactStore,
    KernelSessionSummary,
};

pub mod trajectory;
