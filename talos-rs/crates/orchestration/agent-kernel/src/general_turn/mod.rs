use crate::tool_execution::ToolExecutionContext;
#[cfg(test)]
use crate::UserInput;
use crate::{
    warnings::derive_prompt_warnings, AgentKernelEvent, AgentKernelEventPayload,
    AssistantTextCompletedStreamData, AssistantTextDeltaStreamData, AssistantTextPhase,
    AssistantTextStartedStreamData, InvocationContext, KernelModelCallMode, KernelRunItem,
    KernelStreamItemKey, KernelThinkingSource, KernelThinkingStage, KernelWarning,
    PromptWarningEventData, SessionPromptContextConfig, SessionPromptMessageRole,
    SessionPromptRecentWindow, SessionToolExchangeOutcome, StopReason, ThinkingDeltaStreamData,
    ToolCallStartStreamData, ToolExecutionInput, ToolExecutor, ToolResultStreamData,
    UserActionRequestStreamData,
};
use chrono::Utc;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;
use talos_config::AgentKernelConfig;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{
    ChatMessage, ChatRequest, FunctionCall, LlmProvider, ModelBlockDelta, ModelBlockKind,
    ModelBlockStreamEvent, ModelOutputPhase, ModelSpeed, ReasoningContinuation, ReasoningEffort,
    ReasoningOutput, ToolChoice, ToolDefinition,
};
use talos_memory::{DreamMemoryFormationStore, MemoryScope, MemoryWriteBatch};
use talos_prompt::{
    CompactionTrimUnitKind, ContinuityAnswerMode, PromptBuildConfig, PromptBuilder,
    PromptDiagnostics, PromptLoadPolicy, PromptPhase, PromptWarningState, SessionContinuity,
    SessionHistoryCompaction, SessionHistoryCompactionOutcome, SessionHistoryCompactionTrigger,
};
use talos_tools::{
    ToolDefinitionProvider, ToolDispatchOutcome, ToolExecutorPort, ToolOutputPayload, ToolPending,
};
use talos_types::talos::{
    DynamicToolArguments, StopReason as TalosApiStopReason, TalosContentItem, TokenUsage,
    TurnExecutionMetrics,
};
use talos_types::MemoryGetResponse;
use tokio::sync::mpsc;
use tracing::warn;
use types::{CompactionMemoryOutput, DreamMemoryModelOutput};
use uuid::Uuid;

mod attachments;
mod dream_memory;
mod dream_memory_prompt;
mod finalize;
mod guard;
mod loop_run;
mod memory_summary;
mod persist;
mod prepare;
mod prepare_compaction;
mod prepare_continuity;
mod prepare_load_policy;
#[cfg(test)]
mod prepare_tests;
mod run;
mod step_execute;
mod step_output;
mod step_request;
#[cfg(test)]
mod tests;
mod tool_batch;
mod types;
mod working_context;
#[cfg(test)]
mod working_context_tests;

pub use attachments::build_active_turn_input;

pub use dream_memory::form_dream_session_memory;
pub(crate) use finalize::build_turn_execution_metrics;
pub use finalize::{emit_general_turn_finalization_stream, finalize_general_turn_artifacts};
pub(crate) use guard::{
    build_read_only_nudge_message, build_tool_failure_recovery_nudge_message,
    run_forced_finalization_step, should_force_finalization_for_empty_response,
    should_force_finalization_for_step, should_inject_read_only_nudge,
    stop_reason_for_forced_finalization,
};
pub(crate) use loop_run::execute_general_turn_loop_with_resume;
#[cfg(test)]
pub(crate) use loop_run::{execute_general_turn_loop, execute_general_turn_loop_with_policy};
pub use memory_summary::{
    compact_session_memory, MemoryCompactionPolicy, MemoryFormationParams, MemoryFormationReport,
};
pub use persist::persist_general_turn_state;
pub(crate) use persist::stage_session_state_update;
pub(crate) use prepare::next_session_compaction_state;
pub use prepare::prepare_general_turn;
pub(crate) use prepare_continuity::build_session_continuity;
#[cfg(test)]
pub(crate) use run::run_general_turn;
pub use run::run_general_turn_with_resume;
#[cfg(test)]
pub(crate) use step_execute::stream_round;
pub use step_execute::{apply_general_turn_step, execute_general_turn_step};
pub(crate) use step_output::{emit_assistant_text_stream_item, emit_stream_event};
pub use step_request::build_general_turn_step_request;
#[cfg(test)]
pub(crate) use tool_batch::parse_retrieved_memory_items;
pub use tool_batch::{apply_general_turn_tool_batch, execute_general_turn_tool_batch};
#[cfg(test)]
pub(crate) use types::GeneralTurnPolicyStop;
pub use types::{
    AgentKernelContinuation, AgentKernelContinuationToolCall, AgentKernelUserActionContinuation,
    BuiltInGeneralTurnPolicy, ExecutedTurnArtifacts, GeneralTurnLoopState, GeneralTurnOutcome,
    GeneralTurnPolicy, GeneralTurnResult, GeneralTurnSkillOptions, GeneralTurnStepDisposition,
    GeneralTurnStepRequest, GeneralTurnStepResult, GeneralTurnToolBatch, ModelFinishReason,
    PreparedGeneralTurn, SessionCompactionState, StreamingToolExecutionConfig,
};
pub(crate) use types::{
    ForcedFinalizationCause, PendingUserActionRequest, ReadOnlyRoundFingerprint,
    ToolCallFingerprint, ToolLoopGuardState, ToolResultLog,
};
pub(crate) use working_context::enforce_working_context_budget;
