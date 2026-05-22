use crate::InvocationContext;
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use talos_error::TalosResult;
use talos_llm_provider::{ChatMessage, NativeThinkingConfig, ToolDefinition};
use talos_types::talos::TalosOutputItem;
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
    pub native_thinking: Option<NativeThinkingConfig>,
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
    /// True when the planner detected a completed FINAL_ANSWER in the response.
    #[serde(default)]
    pub has_final_answer: bool,
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
    pub native_thinking: Option<NativeThinkingConfig>,
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
    pub output_items: Vec<TalosOutputItem>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseDisposition {
    Continue,
    Final,
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
mod tests {
    use super::*;
    use crate::{ContextStateBags, InvocationExecutionState, InvocationLimits, TurnPhase};
    use std::collections::HashMap;

    fn mock_ctx() -> InvocationContext {
        InvocationContext {
            session_id: "s1".to_string(),
            message_id: "msg-1".to_string(),
            invocation_id: "i1".to_string(),
            agent_id: "a1".to_string(),
            workspace_id: "w1".to_string(),
            project_root: None,
            input: "hello".to_string(),
            selected_model_info: talos_types::ModelInfo {
                id: "m1".to_string(),
                name: "m1".to_string(),
                provider: "p1".to_string(),
                context_window: 0,
                reasoning: false,
                input: Vec::new(),
                cost: Default::default(),
                compat: Default::default(),
            },
            phase: TurnPhase::Execution,
            planning_instruction: None,
            transfer_to: None,
            execution_state: InvocationExecutionState::default(),
            session: crate::InvocationSession {
                session_id: "s1".to_string(),
                ..crate::InvocationSession::default()
            },
            session_environment: None,
            state: HashMap::new(),
            state_bags: ContextStateBags::default(),
            limits: InvocationLimits::default(),
            events: Vec::new(),
            plugin_manager: crate::default_plugin_manager(),
            transcript_service: None,
            stream_tx: None,
        }
    }

    #[test]
    fn test_final_response_event_matches_adk_style_predicate() {
        let ctx = mock_ctx();
        let final_event =
            ExecutionEvent::final_response(&ctx, ExecutionBranch::ReactiveLoop, "done");
        let tool_event = ExecutionEvent::tool_call(
            &ctx,
            ExecutionBranch::ReactiveLoop,
            "call-1",
            "search",
            serde_json::json!({"q": "talos"}),
        );

        assert!(final_event.is_final_response());
        assert!(!tool_event.is_final_response());
        assert!(!final_event.id.is_empty());
    }

    #[tokio::test]
    async fn test_execution_event_buffer_supports_branch_and_final_queries() {
        let ctx = mock_ctx();
        let mut buffer = ExecutionEventBuffer::new();
        buffer
            .push(ExecutionEvent::text(
                &ctx,
                EventAuthor::User,
                ExecutionBranch::ReactiveLoop,
                ExecutionEventKind::UserInput,
                "hello",
                false,
            ))
            .await
            .unwrap();
        buffer
            .push(ExecutionEvent::final_response(
                &ctx,
                ExecutionBranch::DirectAnswer,
                "done",
            ))
            .await
            .unwrap();

        assert_eq!(buffer.branch_events(ExecutionBranch::ReactiveLoop).len(), 1);
        assert!(buffer.has_final_response(ExecutionBranch::DirectAnswer));
        assert!(!buffer.has_final_response(ExecutionBranch::ReactiveLoop));
        assert!(buffer.last_non_partial_event().is_some());
    }
}
