use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_types::{PendingUserAction, TokenUsage};
use ts_rs::TS;

use crate::{AgentMessage, AgentMessageBlock, AgentToolProgress, AgentTurnRef};

/// Canonical append-only fact emitted during one logical Agent Turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentEvent {
    pub identity: AgentTurnRef,
    #[ts(type = "number")]
    pub sequence: u64,
    #[ts(type = "number")]
    pub occurred_at: i64,
    pub payload: AgentEventPayload,
}

/// Persisted product event exposed by Timeline, history, protocols, and SDKs.
///
/// Service is the only layer allowed to combine Store persistence metadata
/// with the canonical `AgentEvent`. Transport layers may frame this value but
/// must not duplicate its Agent identity, sequence, or timestamp fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimelineEvent {
    pub event_id: String,
    #[ts(type = "number")]
    pub event_seq: i64,
    #[ts(type = "number")]
    pub persisted_at: i64,
    pub event: TimelineEventPayload,
}

/// Timeline combines execution facts and product notices in one durable cursor.
/// Notices never enter the Agent reducer or consume an execution sequence number.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum TimelineEventPayload {
    Agent(AgentEvent),
    SystemNotice(TimelineSystemNotice),
}

/// A product-owned notice attached to a Turn, independent of its execution lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimelineSystemNotice {
    pub identity: AgentTurnRef,
    #[ts(type = "number")]
    pub occurred_at: i64,
    pub notice: crate::AgentSystemNotice,
}

impl TimelineEventPayload {
    /// Identity used for Session authorization and Turn filtering across all facts.
    pub fn identity(&self) -> &AgentTurnRef {
        match self {
            Self::Agent(event) => &event.identity,
            Self::SystemNotice(notice) => &notice.identity,
        }
    }

    /// Execution-only consumers must explicitly exclude product notices.
    pub fn agent_event(&self) -> Option<&AgentEvent> {
        match self {
            Self::Agent(event) => Some(event),
            Self::SystemNotice(_) => None,
        }
    }
}

/// Current nonterminal Turn projection for one Session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimelineActiveTurn {
    pub identity: AgentTurnRef,
    pub initial_message_id: Option<String>,
    pub state: crate::ActiveTurnState,
    pub last_event_id: String,
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// Interpretation of the cursor used for one Timeline replay.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum TimelineCursorStatus {
    Current,
    FromBeginning,
}

/// Stable bounded replay of committed Agent events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TimelineReplay {
    pub events: Vec<TimelineEvent>,
    pub high_water_mark_event_id: Option<String>,
    pub next_cursor: Option<String>,
    #[ts(type = "number")]
    pub last_event_seq: i64,
    pub cursor_status: TimelineCursorStatus,
}

/// Canonical Timeline recovery result shared by every transport.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoverTimelineResponse {
    pub replay: TimelineReplay,
    pub active_turn: Option<TimelineActiveTurn>,
}

/// Canonical response for querying the current nonterminal Turn of a Session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionActiveTurnResponse {
    pub active_turn: Option<TimelineActiveTurn>,
    /// Latest durable Session event, including terminal facts and product notices.
    /// Idle clients use changes to refresh history; this is not a history boundary.
    pub latest_event_id: Option<String>,
}

/// Closed union of canonical Agent Turn lifecycle events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum AgentEventPayload {
    TurnStarted(AgentTurnStarted),
    MessageStarted(AgentMessageStarted),
    MessageBlockStarted(AgentMessageBlockStarted),
    MessageBlockDelta(AgentMessageBlockDelta),
    MessageBlockCompleted(AgentMessageBlockCompleted),
    MessageCompleted(AgentMessage),
    WaitingForInput(AgentPendingInput),
    MetricsUpdated(AgentRunMetrics),
    TurnCompleted(AgentTurnCompleted),
    TurnFailed(AgentTurnFailed),
}

/// Marker payload for the unique start of one logical Turn.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
pub struct AgentTurnStarted {}

/// Started snapshot of one canonical Agent message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentMessageStarted {
    pub message: AgentMessage,
}

/// Started snapshot of one block inside an existing message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentMessageBlockStarted {
    pub message_id: String,
    pub block: AgentMessageBlock,
}

/// Incremental typed update for one running message block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentMessageBlockDelta {
    pub message_id: String,
    pub block_id: String,
    pub delta: AgentMessageBlockDeltaContent,
}

/// Closed union of incremental block updates accepted by the reducer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum AgentMessageBlockDeltaContent {
    Text(String),
    AssistantTextPhase(crate::AgentAssistantTextPhase),
    ToolCallArguments(AgentToolCallArgumentsDelta),
    ToolProgress(AgentToolProgress),
    ContextCompaction(AgentContextCompactionDelta),
}

/// Incremental JSON fragment for one Tool call argument object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentToolCallArgumentsDelta {
    pub json_fragment: String,
}

/// Incremental typed update for a context-compaction block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentContextCompactionDelta {
    pub stage: Option<talos_types::ContextCompactionStage>,
    pub outcome: Option<talos_types::ContextCompactionOutcome>,
    pub metrics: Option<talos_types::ContextCompactionMetrics>,
    pub message: Option<String>,
}

/// Completed snapshot of one block inside an existing message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentMessageBlockCompleted {
    pub message_id: String,
    pub block: AgentMessageBlock,
}

/// Typed input required before the logical Turn can continue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentPendingInput {
    Message { action_id: String, prompt: String },
    UserAction { action: PendingUserAction },
}

impl AgentPendingInput {
    /// Return the canonical action identity used by durable waiting admission.
    pub fn action_id(&self) -> &str {
        match self {
            Self::Message { action_id, .. } => action_id,
            Self::UserAction { action } => &action.action_id,
        }
    }
}

/// Aggregated execution metrics that do not duplicate terminal outcome.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentRunMetrics {
    pub model_rounds: u32,
    pub tool_calls: u32,
    pub tool_successes: u32,
    pub tool_errors: u32,
    pub usage: TokenUsage,
    /// Last measured model context window after this invocation.
    pub context: Option<talos_types::ContextUsage>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<u64>,
}

/// Successful terminal metadata for one logical Turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentTurnCompleted {
    pub stop_reason: AgentStopReason,
}

/// Provider-neutral successful stop reason.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentStopReason {
    EndTurn,
    OutputLimit,
    TransferUnresolved,
}

/// Failed terminal metadata for one logical Turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentTurnFailed {
    pub error: talos_error::ApiError,
    /// Runtime-owned retry policy; not part of the reusable public error object.
    pub retryable: bool,
}

#[cfg(test)]
#[path = "event_tests.rs"]
mod tests;
