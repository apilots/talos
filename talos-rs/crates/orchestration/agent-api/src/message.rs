use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_types::{
    AgentUserContent, ContextCompactionMetrics, ContextCompactionOutcome, ContextCompactionStage,
    ContextCompactionTrigger, TalosContentItem, TalosTimelineNoticeLevel, TalosToolProgressKind,
    ToolArguments,
};
use ts_rs::TS;

/// Canonical message shared by Agent execution, history, protocol, and SDK.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentMessage {
    pub message_id: String,
    pub turn_id: String,
    pub role: AgentMessageRole,
    pub status: AgentMessageStatus,
    pub blocks: Vec<AgentMessageBlock>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number | null")]
    pub completed_at: Option<i64>,
}

/// Author of one canonical Agent message.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentMessageRole {
    User,
    Agent,
    Tool,
    System,
}

/// Lifecycle status of one canonical Agent message.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentMessageStatus {
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

/// One typed lifecycle block inside an Agent message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentMessageBlock {
    pub block_id: String,
    pub status: AgentMessageBlockStatus,
    pub content: AgentMessageBlockContent,
    #[ts(type = "number")]
    pub started_at: i64,
    #[ts(type = "number | null")]
    pub completed_at: Option<i64>,
}

/// Lifecycle status of one Agent message block.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentMessageBlockStatus {
    Queued,
    Running,
    WaitingForInput,
    Completed,
    Failed,
    Cancelled,
}

/// Closed union of content that can appear in canonical Agent history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum AgentMessageBlockContent {
    UserContent(AgentUserContent),
    Reasoning(AgentReasoning),
    AssistantText(AgentAssistantText),
    ToolCall(AgentToolCall),
    ToolResult(AgentToolResult),
    ExecutionSummary(AgentExecutionSummary),
    ContextCompaction(AgentContextCompaction),
    SystemNotice(AgentSystemNotice),
}

/// Model reasoning content safe to expose through the Agent API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentReasoning {
    pub text: String,
}

/// User-visible phase of Agent-authored text.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentAssistantTextPhase {
    Pending,
    Commentary,
    FinalAnswer,
}

/// Agent-authored user-visible text with an explicit presentation phase.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentAssistantText {
    pub phase: AgentAssistantTextPhase,
    pub text: String,
}

/// Typed Tool invocation emitted by an Agent Provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentToolCall {
    pub call_id: String,
    pub name: String,
    pub arguments: AgentToolCallArguments,
    pub progress: Vec<AgentToolProgress>,
}

/// Structured lifecycle of streamed Tool arguments.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentToolCallArguments {
    Streaming { json_fragment: String },
    Complete { value: ToolArguments },
}

/// One typed progress observation for a running Tool call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentToolProgress {
    pub kind: TalosToolProgressKind,
    pub text: String,
    #[ts(type = "number")]
    pub occurred_at: i64,
}

/// Typed Tool result linked to one Tool invocation by `call_id`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentToolResult {
    pub call_id: String,
    pub tool_name: String,
    pub content: Vec<TalosContentItem>,
    pub is_error: bool,
}

/// Bounded execution summary produced during one Agent Turn.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentExecutionSummary {
    pub text: String,
}

/// Typed context-compaction progress or final outcome.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentContextCompaction {
    pub trigger: ContextCompactionTrigger,
    pub stage: ContextCompactionStage,
    pub outcome: Option<ContextCompactionOutcome>,
    pub metrics: Option<ContextCompactionMetrics>,
    pub message: Option<String>,
}

/// Typed system notice emitted as part of Agent execution history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSystemNotice {
    pub level: TalosTimelineNoticeLevel,
    pub message: String,
}

#[cfg(test)]
#[path = "message_tests.rs"]
mod tests;
