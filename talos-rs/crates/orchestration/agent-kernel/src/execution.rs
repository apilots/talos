use crate::InvocationContext;
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use talos_error::TalosResult;
use talos_llm_provider::{ChatMessage, EffectiveReasoning, ToolDefinition};
use talos_types::talos::RunItem;
use talos_types::ModelInfo;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventAuthor {
    User,
    Agent,
    Tool,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionEventKind {
    UserInput,
    Planning,
    Reasoning,
    ToolCall,
    ToolResult,
    FinalResponse,
    System,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum StateNamespace {
    App,
    User,
    Temp,
    Session,
    Workspace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDelta {
    pub namespace: StateNamespace,
    pub key: String,
    pub value: Value,
}

/// Effect produced by the planner's request-phase processing.
#[derive(Debug, Clone, Default)]
pub struct PlanningRequestEffect {
    pub planning_instruction: Option<String>,
    pub clear_request_thoughts: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanningResponseEffect {
    pub normalized_text: Option<String>,
    #[serde(default)]
    pub segments: Vec<PlanningResponseSegment>,
    pub output_text_override: Option<String>,
    #[serde(default)]
    pub clear_output_text: bool,
    #[serde(default)]
    pub state_delta: Vec<StateDelta>,
    pub branch_override: Option<ExecutionBranch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanningResponseSegment {
    pub kind: PlanningResponseSegmentKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanningResponseSegmentKind {
    Thought,
    FinalAnswer,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EventActions {
    #[serde(default)]
    pub state_delta: Vec<StateDelta>,
    #[serde(default)]
    pub end_turn: bool,
    pub transfer_to: Option<String>,
    pub branch_override: Option<ExecutionBranch>,
    pub rewind_before_invocation_id: Option<String>,
    pub compaction: Option<EventCompaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventCompaction {
    pub start_event_id: String,
    pub end_event_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthRequestPayload {
    pub provider: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DynamicToolRequestPayload {
    pub call_id: String,
    pub tool_name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DynamicToolResponsePayload {
    pub call_id: String,
    pub tool_name: String,
    pub output: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionSummaryPayload {
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {
    Text {
        text: String,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments: Value,
    },
    ToolResult {
        call_id: String,
        tool_name: String,
        content: Value,
        #[serde(default)]
        is_error: bool,
    },
    AuthRequest {
        payload: AuthRequestPayload,
    },
    DynamicToolRequest {
        payload: DynamicToolRequestPayload,
    },
    DynamicToolResponse {
        payload: DynamicToolResponsePayload,
    },
    CompactionSummary {
        payload: CompactionSummaryPayload,
    },
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionEvent {
    pub id: String,
    pub invocation_id: String,
    pub author: EventAuthor,
    pub branch: ExecutionBranch,
    pub kind: ExecutionEventKind,
    pub payload: EventPayload,
    #[serde(default)]
    pub actions: EventActions,
    pub timestamp_ms: i64,
    #[serde(default)]
    pub partial: bool,
}

impl ExecutionEvent {
    pub fn new(
        ctx: &InvocationContext,
        author: EventAuthor,
        branch: ExecutionBranch,
        kind: ExecutionEventKind,
        payload: EventPayload,
        partial: bool,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            invocation_id: ctx.invocation_id.clone(),
            author,
            branch,
            kind,
            payload,
            actions: EventActions::default(),
            timestamp_ms: Utc::now().timestamp_millis(),
            partial,
        }
    }

    pub fn text(
        ctx: &InvocationContext,
        author: EventAuthor,
        branch: ExecutionBranch,
        kind: ExecutionEventKind,
        text: impl Into<String>,
        partial: bool,
    ) -> Self {
        Self::new(
            ctx,
            author,
            branch,
            kind,
            EventPayload::Text { text: text.into() },
            partial,
        )
    }

    pub fn final_response(
        ctx: &InvocationContext,
        branch: ExecutionBranch,
        text: impl Into<String>,
    ) -> Self {
        Self::text(
            ctx,
            EventAuthor::Agent,
            branch,
            ExecutionEventKind::FinalResponse,
            text,
            false,
        )
    }

    pub fn tool_call(
        ctx: &InvocationContext,
        branch: ExecutionBranch,
        call_id: impl Into<String>,
        name: impl Into<String>,
        arguments: Value,
    ) -> Self {
        Self::new(
            ctx,
            EventAuthor::Agent,
            branch,
            ExecutionEventKind::ToolCall,
            EventPayload::ToolCall {
                call_id: call_id.into(),
                name: name.into(),
                arguments,
            },
            false,
        )
    }

    pub fn tool_result(
        ctx: &InvocationContext,
        branch: ExecutionBranch,
        call_id: impl Into<String>,
        tool_name: impl Into<String>,
        content: Value,
        is_error: bool,
    ) -> Self {
        Self::new(
            ctx,
            EventAuthor::Tool,
            branch,
            ExecutionEventKind::ToolResult,
            EventPayload::ToolResult {
                call_id: call_id.into(),
                tool_name: tool_name.into(),
                content,
                is_error,
            },
            false,
        )
    }

    pub fn is_final_response(&self) -> bool {
        matches!(self.kind, ExecutionEventKind::FinalResponse)
            && !self.partial
            && !matches!(
                self.payload,
                EventPayload::ToolCall { .. } | EventPayload::ToolResult { .. }
            )
    }
}

#[derive(Debug, Clone)]
pub struct ReplayToolInvocation {
    pub call_id: String,
    pub tool_name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayDynamicToolResult {
    pub output: String,
    pub success: bool,
}

#[derive(Debug, Clone, Default)]
pub struct LlmRequestEnvelope {
    pub messages: Vec<ChatMessage>,
    pub selected_model_info: Option<ModelInfo>,
    pub prompt_prefix_messages: Vec<ChatMessage>,
    pub transient_messages: Vec<ChatMessage>,
    pub planning_instruction: Option<String>,
    pub tool_definitions: Vec<ToolDefinition>,
    pub parallel_tool_calls: bool,
    pub contents_mode: ContentsMode,
    #[allow(dead_code)]
    pub replay_tool_invocations: Vec<ReplayToolInvocation>,
    #[allow(dead_code)]
    pub replay_dynamic_tool_results: HashMap<String, ReplayDynamicToolResult>,
    pub branch: ExecutionBranch,
    pub reasoning: EffectiveReasoning,
    pub cache_context: Option<RequestCacheContext>,
    pub interaction_context: Option<RequestInteractionContext>,
    pub output_contract: RequestOutputContract,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContentsMode {
    CurrentInvocation,
    #[default]
    FullSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestCacheContext {
    pub checkpoint_invocation_id: String,
    pub visible_message_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestInteractionContext {
    pub previous_invocation_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequestOutputContract {
    #[default]
    None,
    ToolMediated,
}

#[derive(Debug, Clone)]
pub struct LlmResponseEnvelope {
    pub output_text: String,
    pub output_items: Vec<RunItem>,
    pub branch: ExecutionBranch,
    pub actions: EventActions,
    pub allow_final_projection: bool,
}

impl Default for LlmResponseEnvelope {
    fn default() -> Self {
        Self {
            output_text: String::new(),
            output_items: Vec::new(),
            branch: ExecutionBranch::default(),
            actions: EventActions::default(),
            allow_final_projection: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionBranch {
    DirectAnswer,
    #[default]
    ReactiveLoop,
    ModelThinking,
}

#[async_trait]
pub trait EventSink: Send {
    async fn push(&mut self, event: ExecutionEvent) -> TalosResult<()>;
}

#[async_trait]
pub trait ExecutionEventHandler: Send {
    async fn handle(
        &mut self,
        ctx: &mut InvocationContext,
        event: ExecutionEvent,
    ) -> TalosResult<Option<ExecutionEvent>>;
}

#[derive(Debug, Default)]
pub struct VecEventSink {
    events: Vec<ExecutionEvent>,
}

impl VecEventSink {
    pub fn into_events(self) -> Vec<ExecutionEvent> {
        self.events
    }
}

#[async_trait]
impl EventSink for VecEventSink {
    async fn push(&mut self, event: ExecutionEvent) -> TalosResult<()> {
        self.events.push(event);
        Ok(())
    }
}

#[derive(Debug, Default, Clone)]
pub struct ExecutionEventBuffer {
    events: Vec<ExecutionEvent>,
}

impl ExecutionEventBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> &[ExecutionEvent] {
        &self.events
    }

    pub fn into_events(self) -> Vec<ExecutionEvent> {
        self.events
    }

    pub fn last_event(&self) -> Option<&ExecutionEvent> {
        self.events.last()
    }

    pub fn last_non_partial_event(&self) -> Option<&ExecutionEvent> {
        self.events.iter().rev().find(|event| !event.partial)
    }

    pub fn branch_events(&self, branch: ExecutionBranch) -> Vec<&ExecutionEvent> {
        self.events
            .iter()
            .filter(|event| event.branch == branch)
            .collect()
    }

    pub fn has_final_response(&self, branch: ExecutionBranch) -> bool {
        self.events
            .iter()
            .rev()
            .any(|event| event.branch == branch && event.is_final_response())
    }
}

#[async_trait]
impl EventSink for ExecutionEventBuffer {
    async fn push(&mut self, event: ExecutionEvent) -> TalosResult<()> {
        self.events.push(event);
        Ok(())
    }
}

#[cfg(test)]
#[path = "execution_tests.rs"]
mod tests;
