use crate::tool_execution::ToolExecutionContext;
use crate::{
    warnings::derive_prompt_warnings, AssistantTextCompletedStreamData,
    AssistantTextDeltaStreamData, AssistantTextStartedStreamData, DynamicToolRequestStreamData,
    InvocationContext, KernelAssistantTextPhase, KernelEvent, KernelEventData, KernelEventLabel,
    KernelEventType, KernelModelCallMode, KernelStreamEvent, KernelStreamEventData,
    KernelStreamEventType, KernelStreamItemKey, KernelThinkingSource, KernelThinkingStage,
    KernelWarning, PromptWarningEventData, SessionPromptContextConfig, SessionPromptRecentWindow,
    SessionToolExchangeOutcome, StopReason, TalosMessageRole, ThinkingDeltaStreamData,
    ToolCallStartStreamData, ToolExecutionInput, ToolExecutor, ToolResultStreamData, UserInput,
};
use chrono::Utc;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;
use talos_config::AgentKernelConfig;
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{
    AssistantTextPhaseHint, ChatMessage, ChatRequest, FunctionCall, LlmProvider, ModelBlockDelta,
    ModelBlockKind, ModelBlockStreamEvent, ModelSpeed, ReasoningContinuation, ReasoningEffort,
    ReasoningOutput, ToolChoice, ToolDefinition,
};
use talos_memory::{MemoryScope, MemoryWriteBatch, MemoryWriter};
use talos_prompt::{
    CompactionTrimUnitKind, ContinuityAnswerMode, PromptBuildConfig, PromptBuilder,
    PromptDiagnostics, PromptLoadPolicy, PromptPhase, PromptWarningState, SessionContinuity,
    SessionHistoryCompaction, SessionHistoryCompactionOutcome, SessionHistoryCompactionTrigger,
};
use talos_store::session::{RecordHeader, SummaryRecord};
use talos_tools::{
    ToolDefinitionProvider, ToolDispatchOutcome, ToolExecutorPort, ToolOutputPayload, ToolPending,
};
use talos_types::talos::{
    DynamicToolArguments, RunItem, StopReason as TalosApiStopReason, TalosContentItem, TokenUsage,
    TurnExecutionMetrics,
};
use talos_types::MemoryGetResponse;
use tokio::sync::mpsc;
use tracing::warn;
use types::CompactionMemoryOutput;
use uuid::Uuid;

const MEMORY_COMPACTION_PROMPT: &str = r#"Extract durable memory from this conversation as JSON.
Return ONLY valid JSON, no markdown fences, no explanation.

Schema:
{
  "title": "one-line core topic, max 12 words, no trailing punctuation",
  "facts": ["..."],
  "preferences": ["..."],
  "open_tasks": ["..."],
  "risks": ["..."],
  "memory_candidates": [
    {
      "title": "durable memory title",
      "body": "durable fact, preference, decision, task, or episode",
      "kind": "preference" | "fact" | "decision" | "task" | "episode",
      "importance": "low" | "normal" | "high"
    }
  ]
}

Omit any field whose array would be empty. Keep total under 200 words.
Return at most 8 memory_candidates. Exclude transient logs and unstable details.
Use the same language as the conversation transcript for all field values."#;

mod attachments;
mod daily_memory;
mod finalize;
mod guard;
mod loop_run;
mod memory_summary;
mod persist;
mod prepare;
mod prepare_compaction;
mod prepare_continuity;
#[cfg(test)]
mod prepare_tests;
mod run;
mod step_execute;
mod step_request;
#[cfg(test)]
mod tests;
mod tool_batch;
mod types;
mod working_context;
#[cfg(test)]
mod working_context_tests;

pub use daily_memory::form_daily_session_memory;
pub(crate) use finalize::{build_turn_execution_metrics, to_api_stop_reason};
pub use finalize::{emit_general_turn_finalization_stream, finalize_general_turn_artifacts};
pub(crate) use guard::{
    build_read_only_nudge_message, build_tool_failure_recovery_nudge_message,
    run_forced_finalization_step, should_force_finalization_for_empty_response,
    should_force_finalization_for_step, should_inject_read_only_nudge,
    stop_reason_for_forced_finalization,
};
pub use loop_run::{
    execute_general_turn_loop, execute_general_turn_loop_with_policy,
    execute_general_turn_loop_with_resume,
};
pub use memory_summary::{
    compact_session_memory, MemoryCompactionPolicy, MemoryFormationParams, MemoryFormationReport,
};
pub use persist::persist_general_turn_artifacts;
pub use prepare::prepare_general_turn;
pub(crate) use prepare::{next_session_compaction_state, stage_session_state_update};
pub(crate) use prepare::{
    LAST_PROMPT_DIAGNOSTICS_STATE_KEY, PROMPT_WARNING_STATE_KEY, SESSION_CONTINUITY_STATE_KEY,
};
pub use prepare_compaction::SESSION_COMPACTION_STATE_KEY;
pub(crate) use prepare_continuity::build_session_continuity;
pub use run::{run_general_turn, run_general_turn_with_resume};
#[cfg(test)]
pub(crate) use step_execute::stream_round;
pub use step_execute::{apply_general_turn_step, execute_general_turn_step};
pub(crate) use step_execute::{emit_assistant_text_stream_item, emit_stream_event};
pub use step_request::build_general_turn_step_request;
#[cfg(test)]
pub(crate) use tool_batch::parse_retrieved_memory_items;
pub use tool_batch::{apply_general_turn_tool_batch, execute_general_turn_tool_batch};
pub use types::{
    BuiltInGeneralTurnPolicy, DynamicToolResultResume, ExecutedTurnArtifacts, GeneralTurnLoopState,
    GeneralTurnOutcome, GeneralTurnPolicy, GeneralTurnPolicyStop, GeneralTurnResult,
    GeneralTurnResume, GeneralTurnSkillOptions, GeneralTurnStepDisposition, GeneralTurnStepRequest,
    GeneralTurnStepResult, GeneralTurnToolBatch, ModelFinishReason, PendingGeneralTurnAction,
    PreparedGeneralTurn, ResumeToolCall, SessionCompactionState, StreamingToolExecutionConfig,
};
pub(crate) use types::{
    ForcedFinalizationCause, PendingDynamicToolRequest, ReadOnlyRoundFingerprint,
    ToolCallFingerprint, ToolLoopGuardState, ToolResultLog,
};
pub(crate) use working_context::enforce_working_context_budget;
