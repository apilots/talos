use crate::tool_execution::ToolExecutionContext;
use crate::{
    warnings::derive_prompt_warnings, ExecutionSummaryStreamData, InvocationContext, KernelEvent,
    KernelEventData, KernelEventLabel, KernelEventType, KernelOutputDeltaSource, KernelStreamEvent,
    KernelStreamEventData, KernelStreamEventType, KernelThinkingSource, KernelThinkingStage,
    KernelWarning, OutputDeltaStreamData, PromptWarningEventData, StopReason,
    ThinkingDeltaStreamData, ToolCallStartStreamData, ToolExecutionInput, ToolExecutor,
    ToolResultStreamData,
};
use chrono::Utc;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_llm_provider::{
    ChatMessage, ChatRequest, FunctionCall, LlmProvider, NativeThinkingConfig, ToolChoice,
    ToolDefinition, ToolStreamEvent,
};
use talos_memory::{
    strip_json_fences, Memory, MemoryGetResult, MemorySearchResponse, MemoryStore, RecordHeader,
    RetrievedMemoryItem, SessionMemorySummary, SessionPromptContextConfig, SummaryRecord,
    TalosMessageRole, TalosTurn, ToolCallRecord,
};
use talos_prompt::{
    render_prompt_assembly, PromptBudget, PromptBuildConfig, PromptBuilder, PromptDiagnostics,
    PromptLoadPolicy, PromptPhase,
};
use talos_tools::{
    ToolDefinitionProvider, ToolDispatchOutcome, ToolExecutorPort, ToolOutputPayload, ToolPending,
};
use talos_types::talos::{
    StopReason as TalosApiStopReason, TalosContentItem, TalosOutputItem, TalosUsage,
};
use tokio::sync::mpsc;
use tracing::warn;
use uuid::Uuid;

const MEMORY_SUMMARY_PROMPT: &str = r#"Extract durable memory from this conversation as JSON.
Return ONLY valid JSON, no markdown fences, no explanation.

Schema:
{
  "title": "one-line core topic, max 12 words, no trailing punctuation",
  "facts": ["..."],
  "preferences": ["..."],
  "open_tasks": ["..."],
  "risks": ["..."]
}

Omit any field whose array would be empty. Keep total under 200 words.
Use the same language as the conversation transcript for all field values."#;

#[cfg(test)]
mod convergence_tests;
mod finalize;
mod guard;
mod memory_summary;
mod prepare;
mod step_execute;
mod step_request;
#[cfg(test)]
mod tests;
mod tool_batch;
mod types;

pub(crate) use finalize::to_api_stop_reason;
pub use finalize::{emit_general_turn_finalization_stream, finalize_general_turn_artifacts};
pub(crate) use guard::{
    is_read_only_tool_round, run_forced_finalization_step,
    should_force_finalization_for_empty_response, should_force_finalization_for_step,
    stop_reason_for_forced_finalization,
};
pub(crate) use memory_summary::compact_workspace_context_if_needed;
pub use memory_summary::{flush_session_memory, MemoryFlushPolicy};
pub use prepare::{
    execute_general_turn_loop, persist_general_turn_artifacts, prepare_general_turn,
    run_general_turn,
};
pub(crate) use step_execute::emit_stream_event;
#[cfg(test)]
pub(crate) use step_execute::stream_round;
pub use step_execute::{apply_general_turn_step, execute_general_turn_step};
pub use step_request::build_general_turn_step_request;
pub(crate) use tool_batch::collect_general_turn_tool_batch;
#[cfg(test)]
pub(crate) use tool_batch::parse_retrieved_memory_items;
pub use tool_batch::{apply_general_turn_tool_batch, execute_general_turn_tool_batch};
pub use types::{
    ExecutedTurnArtifacts, GeneralTurnLoopState, GeneralTurnResult, GeneralTurnStepDisposition,
    GeneralTurnStepRequest, GeneralTurnStepResult, GeneralTurnToolBatch, ModelFinishReason,
    PlanReActLoopState, PreparedGeneralTurn, StreamingToolExecutionConfig,
};
pub(crate) use types::{
    ForcedFinalizationCause, PendingDynamicToolRequest, PendingToolConfirmation,
    ReadOnlyRoundFingerprint, ToolCallFingerprint, ToolLoopGuardState, ToolResultLog,
};
