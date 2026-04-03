use crate::execution::ContentsMode;
use crate::{
    execution::{PlanningResponseSegmentKind, ReplayDynamicToolResult, ReplayToolInvocation},
    CompactionSummaryPayload, ContentsBuilder, EventActions, EventAuthor, EventPayload,
    ExecutionEvent, ExecutionEventKind, FlowProfile, InvocationContext, LlmRequestEnvelope,
    LlmResponseEnvelope, Planner, RequestOutputContract, ResponseDisposition, SessionEvent,
    SessionEventPayload,
};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_llm_provider::{ChatMessage, Role};
use talos_types::talos::{TalosContentItem, TalosOutputItem};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RequestProcessorKind {
    Basic = 0,
    Auth = 1,
    RequestConfirmation = 2,
    DynamicToolReplay = 3,
    Instructions = 4,
    Identity = 5,
    Compaction = 6,
    Contents = 7,
    ContextCache = 8,
    Interactions = 9,
    OutputSchema = 10,
    Planning = 11,
    ToolContext = 12,
    AgentTransfer = 13,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ResponseProcessorKind {
    Planning = 0,
    ToolCallExtraction = 1,
    FinalAnswerExtraction = 2,
    StateDelta = 3,
    EventProjection = 4,
    CodeExecution = 5,
}

#[async_trait]
pub trait RequestProcessor: Send + Sync {
    fn kind(&self) -> RequestProcessorKind;

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        _request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
pub trait ResponseProcessor: Send + Sync {
    fn kind(&self) -> ResponseProcessorKind;

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        _response: &mut LlmResponseEnvelope,
    ) -> TalosResult<ResponseProcessorOutput> {
        Ok(ResponseProcessorOutput::default())
    }
}

#[derive(Debug, Default, Clone)]
pub struct RequestProcessorOutput {
    pub events: Vec<ExecutionEvent>,
}

#[derive(Debug, Clone)]
pub struct ResponseProcessorOutput {
    pub disposition: ResponseDisposition,
    pub events: Vec<ExecutionEvent>,
}

impl Default for ResponseProcessorOutput {
    fn default() -> Self {
        Self {
            disposition: ResponseDisposition::Continue,
            events: Vec::new(),
        }
    }
}

pub struct BasicRequestProcessor;

pub struct InstructionsRequestProcessor;
pub struct AuthRequestProcessor;
pub struct RequestConfirmationRequestProcessor;
pub struct DynamicToolReplayRequestProcessor;
pub struct IdentityRequestProcessor {
    agent_name: String,
    agent_description: String,
}
pub struct CompactionRequestProcessor;
pub struct ContextCacheRequestProcessor;
pub struct InteractionsRequestProcessor;
pub struct OutputSchemaRequestProcessor;

#[derive(Debug, Clone, PartialEq, Eq)]
struct InvocationCompactionGroup {
    invocation_id: String,
    end_index: usize,
    total_bytes: usize,
}

impl IdentityRequestProcessor {
    pub fn new(agent_name: impl Into<String>, agent_description: impl Into<String>) -> Self {
        Self {
            agent_name: agent_name.into(),
            agent_description: agent_description.into(),
        }
    }
}

fn stage_request_event(ctx: &mut InvocationContext, event: &ExecutionEvent) {
    let staged = SessionEvent::from_execution_event(ctx.session_id.clone(), event.clone());
    for delta in &staged.actions.state_delta {
        ctx.session.state.apply_delta(delta);
    }
    ctx.session.events.push(staged);
}

fn is_context_visible_event(event: &SessionEvent) -> bool {
    !event.partial
        && !matches!(
            event.payload,
            SessionEventPayload::StateOnly
                | SessionEventPayload::Transfer { .. }
                | SessionEventPayload::Auth { .. }
                | SessionEventPayload::Confirmation { .. }
        )
        && event.payload.message().is_some_and(|message| {
            !message.content.trim().is_empty() || message.tool_calls.is_some()
        })
}

fn message_size(message: &ChatMessage) -> usize {
    let tool_call_bytes = message
        .tool_calls
        .as_ref()
        .map(|calls| calls.len() * 96)
        .unwrap_or_default();
    message.content.len() + tool_call_bytes + usize::from(message.tool_call_id.is_some()) * 32
}

fn group_events_by_invocation(events: &[SessionEvent]) -> Vec<InvocationCompactionGroup> {
    let mut groups: Vec<InvocationCompactionGroup> = Vec::new();
    for (index, event) in events.iter().enumerate() {
        let event_bytes = event
            .payload
            .message()
            .map(message_size)
            .unwrap_or_default();
        match groups.last_mut() {
            Some(group) if group.invocation_id == event.invocation_id => {
                group.end_index = index;
                group.total_bytes += event_bytes;
            }
            _ => groups.push(InvocationCompactionGroup {
                invocation_id: event.invocation_id.clone(),
                end_index: index,
                total_bytes: event_bytes,
            }),
        }
    }
    groups
}

fn truncate_for_summary(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let compact = trimmed.chars().take(limit).collect::<String>();
    format!("{}...", compact)
}

fn summarize_compacted_events(events: &[SessionEvent]) -> Option<String> {
    if events.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for event in events.iter().take(6) {
        let Some(message) = event.payload.message() else {
            continue;
        };
        let speaker = match message.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        let body = if let Some(tool_calls) = &message.tool_calls {
            let tools = tool_calls
                .iter()
                .map(|call| call.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!("tool calls: {}", tools)
        } else {
            truncate_for_summary(&message.content, 120)
        };
        lines.push(format!("- {}: {}", speaker, body));
    }
    if lines.is_empty() {
        None
    } else {
        Some(format!("Compacted previous context:\n{}", lines.join("\n")))
    }
}

fn latest_branch_event<'a>(
    ctx: &'a InvocationContext,
    branch: crate::ExecutionBranch,
    predicate: impl Fn(&'a SessionEvent) -> bool,
) -> Option<&'a SessionEvent> {
    ctx.session
        .events
        .iter()
        .rev()
        .find(|event| event.branch == branch && predicate(event))
}

#[async_trait]
impl RequestProcessor for InstructionsRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Instructions
    }

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        if request.prompt_prefix_messages.is_empty() {
            if let Some(instruction) = &request.planning_instruction {
                request
                    .prompt_prefix_messages
                    .push(talos_llm_provider::ChatMessage::system(instruction.clone()));
            }
        }
        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for AuthRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Auth
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        let latest_auth = latest_branch_event(ctx, request.branch, |event| {
            matches!(event.payload, SessionEventPayload::Auth { .. })
        });
        let latest_user = latest_branch_event(ctx, request.branch, |event| {
            matches!(event.payload, SessionEventPayload::Message { .. })
                && matches!(event.author, EventAuthor::User)
        });
        if let Some(event) = latest_auth {
            let auth_is_pending = latest_user
                .map(|user_event| user_event.timestamp <= event.timestamp)
                .unwrap_or(true);
            if auth_is_pending {
                if let SessionEventPayload::Auth { payload } = &event.payload {
                    request.prompt_prefix_messages.push(ChatMessage::system(format!(
                        "Authentication is still pending for provider `{}`. Only continue once the credential challenge has been resolved. Context: {}",
                        payload.provider, payload.prompt
                    )));
                }
            }
        }
        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for RequestConfirmationRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::RequestConfirmation
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        let latest_confirmation = latest_branch_event(ctx, request.branch, |event| {
            matches!(event.payload, SessionEventPayload::Confirmation { .. })
        });

        let Some(confirmation_event) = latest_confirmation else {
            return Ok(RequestProcessorOutput::default());
        };

        let SessionEventPayload::Confirmation {
            payload: confirmation,
        } = &confirmation_event.payload
        else {
            return Ok(RequestProcessorOutput::default());
        };

        // Find the latest ApprovalResponse for this specific call_id.
        let latest_approval = latest_branch_event(ctx, request.branch, |event| {
            if let SessionEventPayload::ApprovalResponse { payload } = &event.payload {
                payload.call_id == confirmation.call_id
            } else {
                false
            }
        });

        match latest_approval {
            Some(approval_event) if approval_event.timestamp > confirmation_event.timestamp => {
                let SessionEventPayload::ApprovalResponse { payload: approval } =
                    &approval_event.payload
                else {
                    return Ok(RequestProcessorOutput::default());
                };
                if approval.approved {
                    // Inject replay invocation so the flow executes the tool with approval bypass.
                    request.replay_tool_invocations.push(ReplayToolInvocation {
                        call_id: confirmation.call_id.clone(),
                        tool_name: confirmation.tool_name.clone(),
                        arguments: confirmation.arguments.clone(),
                    });
                } else {
                    let reason = approval.reason.as_deref().unwrap_or("no reason given");
                    request.prompt_prefix_messages.push(ChatMessage::system(format!(
                        "Tool `{}` (call `{}`) was rejected by the user: {}. Inform the user and continue.",
                        approval.tool_name, approval.call_id, reason
                    )));
                }
            }
            _ => {
                // No approval response yet — still pending.
                request.prompt_prefix_messages.push(ChatMessage::system(format!(
                    "A tool confirmation is pending for `{}` (call `{}`). Wait for explicit user approval before continuing. Prompt: {}",
                    confirmation.tool_name, confirmation.call_id, confirmation.prompt
                )));
            }
        }

        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for DynamicToolReplayRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::DynamicToolReplay
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        let latest_dynamic_request = latest_branch_event(ctx, request.branch, |event| {
            matches!(
                event.payload,
                SessionEventPayload::DynamicToolRequest { .. }
            )
        });

        let Some(dynamic_request_event) = latest_dynamic_request else {
            return Ok(RequestProcessorOutput::default());
        };

        let SessionEventPayload::DynamicToolRequest {
            payload: dynamic_request,
        } = &dynamic_request_event.payload
        else {
            return Ok(RequestProcessorOutput::default());
        };

        let latest_dynamic_response = latest_branch_event(ctx, request.branch, |event| {
            if let SessionEventPayload::DynamicToolResponse { payload } = &event.payload {
                payload.call_id == dynamic_request.call_id
            } else {
                false
            }
        });

        match latest_dynamic_response {
            Some(response_event) if response_event.timestamp > dynamic_request_event.timestamp => {
                let SessionEventPayload::DynamicToolResponse { payload } = &response_event.payload
                else {
                    return Ok(RequestProcessorOutput::default());
                };
                request.replay_tool_invocations.push(ReplayToolInvocation {
                    call_id: payload.call_id.clone(),
                    tool_name: payload.tool_name.clone(),
                    arguments: dynamic_request.arguments.clone(),
                });
                request.replay_dynamic_tool_results.insert(
                    payload.call_id.clone(),
                    ReplayDynamicToolResult {
                        output: payload.output.clone(),
                        success: payload.success,
                    },
                );
            }
            _ => {
                let timeout_ms = ctx.limits.dynamic_tool_timeout_ms;
                let elapsed_ms = (Utc::now() - dynamic_request_event.timestamp)
                    .num_milliseconds()
                    .max(0) as u64;
                if timeout_ms > 0 && elapsed_ms >= timeout_ms {
                    let timeout_message = format!(
                        "dynamic tool `{}` (call `{}`) timed out after {} ms",
                        dynamic_request.tool_name, dynamic_request.call_id, timeout_ms
                    );
                    match ctx.limits.dynamic_tool_timeout_behavior {
                        talos_config::DynamicToolTimeoutBehavior::ErrorToModel => {
                            request.replay_tool_invocations.push(ReplayToolInvocation {
                                call_id: dynamic_request.call_id.clone(),
                                tool_name: dynamic_request.tool_name.clone(),
                                arguments: dynamic_request.arguments.clone(),
                            });
                            request.replay_dynamic_tool_results.insert(
                                dynamic_request.call_id.clone(),
                                ReplayDynamicToolResult {
                                    output: timeout_message,
                                    success: false,
                                },
                            );
                        }
                        talos_config::DynamicToolTimeoutBehavior::AbortTurn => {
                            request.prompt_prefix_messages.push(ChatMessage::system(format!(
                                "Stop the turn and report that dynamic tool `{}` (call `{}`) timed out.",
                                dynamic_request.tool_name, dynamic_request.call_id
                            )));
                        }
                    }
                } else {
                    request.prompt_prefix_messages.push(ChatMessage::system(format!(
                        "A dynamic tool response is pending for `{}` (call `{}`). Wait for the external tool result before continuing.",
                        dynamic_request.tool_name, dynamic_request.call_id
                    )));
                }
            }
        }

        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for BasicRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Basic
    }

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        request.parallel_tool_calls =
            request.parallel_tool_calls && !request.tool_definitions.is_empty();
        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for IdentityRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Identity
    }

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        if request.prompt_prefix_messages.is_empty() {
            let mut instruction = format!(
                "You are an agent. Your internal name is \"{}\".",
                self.agent_name
            );
            if !self.agent_description.trim().is_empty() {
                instruction.push_str(&format!(
                    " The description about you is \"{}\".",
                    self.agent_description
                ));
            }
            request
                .prompt_prefix_messages
                .push(ChatMessage::system(instruction));
        }
        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for CompactionRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Compaction
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        let threshold = ctx.limits.context_compaction_max_bytes;
        if threshold == 0 {
            return Ok(RequestProcessorOutput::default());
        }

        let visible_events = ctx
            .session
            .filter_events(None, Some(request.branch))
            .into_iter()
            .filter(|event| event.invocation_id != ctx.invocation_id)
            .filter(is_context_visible_event)
            .collect::<Vec<_>>();
        let invocation_groups = group_events_by_invocation(&visible_events);
        if invocation_groups.len() <= 1 {
            return Ok(RequestProcessorOutput::default());
        }

        let total_bytes = visible_events
            .iter()
            .filter_map(|event| event.payload.message())
            .map(message_size)
            .sum::<usize>();
        if total_bytes <= threshold {
            return Ok(RequestProcessorOutput::default());
        }

        let target_tail_bytes = threshold / 2;
        let mut keep_group_start = invocation_groups.len();
        let mut tail_bytes = 0usize;
        for (index, group) in invocation_groups.iter().enumerate().rev() {
            tail_bytes += group.total_bytes;
            let retained_groups = invocation_groups.len() - index;
            if retained_groups >= 1 && tail_bytes >= target_tail_bytes {
                keep_group_start = index;
                break;
            }
        }

        if keep_group_start == 0 || keep_group_start >= invocation_groups.len() {
            return Ok(RequestProcessorOutput::default());
        }

        let compacted_end_index = invocation_groups[keep_group_start - 1].end_index;
        let compacted_slice = &visible_events[..=compacted_end_index];
        let Some(summary) = summarize_compacted_events(compacted_slice) else {
            return Ok(RequestProcessorOutput::default());
        };
        let mut compaction_event = ExecutionEvent::new(
            ctx,
            EventAuthor::System,
            request.branch,
            ExecutionEventKind::System,
            EventPayload::CompactionSummary {
                payload: CompactionSummaryPayload { summary },
            },
            false,
        );
        compaction_event.actions.compaction = Some(crate::execution::EventCompaction {
            start_event_id: compacted_slice
                .first()
                .expect("compaction slice should not be empty")
                .id
                .clone(),
            end_event_id: compacted_slice
                .last()
                .expect("compaction slice should not be empty")
                .id
                .clone(),
        });

        Ok(RequestProcessorOutput {
            events: vec![compaction_event],
        })
    }
}

pub struct ContentsRequestProcessor;

#[async_trait]
impl RequestProcessor for ContentsRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Contents
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        let mut messages = request.prompt_prefix_messages.clone();
        messages.extend(ContentsBuilder::build_messages(
            &ctx.session,
            &ctx.invocation_id,
            request.branch,
            request.contents_mode,
        ));
        messages.extend(request.transient_messages.clone());
        request.messages = messages;
        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for ContextCacheRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::ContextCache
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        let visible_messages = ContentsBuilder::build_messages(
            &ctx.session,
            &ctx.invocation_id,
            request.branch,
            request.contents_mode,
        );
        if let Some(checkpoint) = ctx
            .session
            .events
            .iter()
            .rev()
            .find(|event| event.branch == request.branch && !event.partial)
        {
            request.cache_context = Some(crate::RequestCacheContext {
                checkpoint_invocation_id: checkpoint.invocation_id.clone(),
                visible_message_count: visible_messages.len(),
            });
        }
        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for InteractionsRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Interactions
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        if let Some(previous_agent_event) = ctx.session.events.iter().rev().find(|event| {
            event.branch == request.branch
                && !event.partial
                && matches!(event.author, EventAuthor::Agent)
                && matches!(event.payload, SessionEventPayload::Message { .. })
        }) {
            request.interaction_context = Some(crate::RequestInteractionContext {
                previous_invocation_id: previous_agent_event.invocation_id.clone(),
            });
        }
        Ok(RequestProcessorOutput::default())
    }
}

#[async_trait]
impl RequestProcessor for OutputSchemaRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::OutputSchema
    }

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        if request.tool_definitions.is_empty() {
            return Ok(RequestProcessorOutput::default());
        }
        request.output_contract = RequestOutputContract::ToolMediated;
        request.prompt_prefix_messages.push(ChatMessage::system(
            "When tools are required, finish the tool work first and then return a final answer that is explicit and complete."
                .to_string(),
        ));
        Ok(RequestProcessorOutput::default())
    }
}

pub struct ToolContextRequestProcessor;

#[async_trait]
impl RequestProcessor for ToolContextRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::ToolContext
    }

    async fn run(
        &self,
        _ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        if matches!(request.branch, crate::ExecutionBranch::DirectAnswer) {
            request.tool_definitions.clear();
            request.parallel_tool_calls = false;
            return Ok(RequestProcessorOutput::default());
        }
        if matches!(request.contents_mode, ContentsMode::CurrentInvocation) {
            request.parallel_tool_calls =
                request.parallel_tool_calls && !request.tool_definitions.is_empty();
        }
        Ok(RequestProcessorOutput::default())
    }
}

pub struct AgentTransferRequestProcessor;

#[async_trait]
impl RequestProcessor for AgentTransferRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::AgentTransfer
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        if let Some(transfer_to) = ctx.transfer_to.clone() {
            if transfer_to.trim().is_empty() {
                ctx.transfer_to = None;
                return Ok(RequestProcessorOutput::default());
            }

            request.prompt_prefix_messages.push(talos_llm_provider::ChatMessage::system(
                format!(
                    "Route this turn to agent `{}` when handoff is required. Preserve tool context until transfer is resolved.",
                    transfer_to
                ),
            ));

            let mut transfer_event = ExecutionEvent::new(
                ctx,
                EventAuthor::System,
                request.branch,
                ExecutionEventKind::Planning,
                EventPayload::Empty,
                false,
            );
            transfer_event.actions.transfer_to = Some(transfer_to);
            return Ok(RequestProcessorOutput {
                events: vec![transfer_event],
            });
        }
        Ok(RequestProcessorOutput::default())
    }
}

pub struct PlanningRequestProcessor {
    planner: Arc<dyn Planner>,
}

impl PlanningRequestProcessor {
    pub fn new(planner: Arc<dyn Planner>) -> Self {
        Self { planner }
    }
}

#[async_trait]
impl RequestProcessor for PlanningRequestProcessor {
    fn kind(&self) -> RequestProcessorKind {
        RequestProcessorKind::Planning
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<RequestProcessorOutput> {
        let effect = self.planner.build_request_effect(ctx, request);
        request.native_thinking = effect.native_thinking;
        ctx.planning_instruction = effect.planning_instruction.clone();
        request.planning_instruction = effect.planning_instruction;
        Ok(RequestProcessorOutput::default())
    }
}

pub struct PlanningResponseProcessor {
    planner: Arc<dyn Planner>,
}

impl PlanningResponseProcessor {
    pub fn new(planner: Arc<dyn Planner>) -> Self {
        Self { planner }
    }
}

#[async_trait]
impl ResponseProcessor for PlanningResponseProcessor {
    fn kind(&self) -> ResponseProcessorKind {
        ResponseProcessorKind::Planning
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        response: &mut LlmResponseEnvelope,
    ) -> TalosResult<ResponseProcessorOutput> {
        let effect = self.planner.process_response_effect(ctx, response);

        ctx.set_plan_react_has_final_answer(effect.has_final_answer);
        if let Some(normalized) = effect.normalized_text {
            ctx.set_planning_response(normalized);
        }

        for segment in &effect.segments {
            match segment.kind {
                PlanningResponseSegmentKind::Thought => {
                    response.output_items.push(TalosOutputItem::Reasoning {
                        text: segment.text.clone(),
                    });
                }
                PlanningResponseSegmentKind::FinalAnswer => {}
            }
        }

        if let Some(output_text_override) = effect.output_text_override {
            response.output_text = output_text_override;
        } else if effect.clear_output_text {
            response.output_text.clear();
        }

        if !effect.state_delta.is_empty() || effect.branch_override.is_some() {
            if let Some(branch_override) = effect.branch_override {
                response.branch = branch_override;
            }
            response.actions = EventActions {
                state_delta: effect.state_delta.clone(),
                end_turn: false,
                transfer_to: None,
                branch_override: effect.branch_override,
                rewind_before_invocation_id: None,
                compaction: None,
            };
            let mut event = ExecutionEvent::new(
                ctx,
                EventAuthor::Agent,
                response.branch,
                ExecutionEventKind::Planning,
                EventPayload::Empty,
                false,
            );
            event.actions = response.actions.clone();
            return Ok(ResponseProcessorOutput {
                disposition: ResponseDisposition::Continue,
                events: vec![event],
            });
        }

        Ok(ResponseProcessorOutput::default())
    }
}

pub struct ToolCallExtractionProcessor;

#[async_trait]
impl ResponseProcessor for ToolCallExtractionProcessor {
    fn kind(&self) -> ResponseProcessorKind {
        ResponseProcessorKind::ToolCallExtraction
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        response: &mut LlmResponseEnvelope,
    ) -> TalosResult<ResponseProcessorOutput> {
        let mut events = Vec::new();
        for item in &response.output_items {
            if let TalosOutputItem::ToolCall {
                call_id,
                name,
                arguments,
            } = item
            {
                events.push(ExecutionEvent::tool_call(
                    ctx,
                    response.branch,
                    call_id.clone(),
                    name.clone(),
                    arguments.clone(),
                ));
            }
        }

        Ok(ResponseProcessorOutput {
            disposition: ResponseDisposition::Continue,
            events,
        })
    }
}

pub struct FinalAnswerExtractionProcessor;

#[async_trait]
impl ResponseProcessor for FinalAnswerExtractionProcessor {
    fn kind(&self) -> ResponseProcessorKind {
        ResponseProcessorKind::FinalAnswerExtraction
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        response: &mut LlmResponseEnvelope,
    ) -> TalosResult<ResponseProcessorOutput> {
        if !response.allow_final_projection {
            return Ok(ResponseProcessorOutput::default());
        }
        let has_tool_calls = response
            .output_items
            .iter()
            .any(|item| matches!(item, TalosOutputItem::ToolCall { .. }));
        if has_tool_calls {
            return Ok(ResponseProcessorOutput::default());
        }
        let mut emitted = false;
        let mut events = Vec::new();

        for item in &response.output_items {
            if let TalosOutputItem::OutputText { text } = item {
                events.push(ExecutionEvent::final_response(
                    ctx,
                    response.branch,
                    text.clone(),
                ));
                emitted = true;
            }
        }

        if !emitted && !response.output_text.trim().is_empty() {
            events.push(ExecutionEvent::final_response(
                ctx,
                response.branch,
                response.output_text.clone(),
            ));
            emitted = true;
        }

        if emitted {
            Ok(ResponseProcessorOutput {
                disposition: ResponseDisposition::Final,
                events,
            })
        } else {
            Ok(ResponseProcessorOutput::default())
        }
    }
}

pub struct EventProjectionProcessor;

#[async_trait]
impl ResponseProcessor for EventProjectionProcessor {
    fn kind(&self) -> ResponseProcessorKind {
        ResponseProcessorKind::EventProjection
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        response: &mut LlmResponseEnvelope,
    ) -> TalosResult<ResponseProcessorOutput> {
        let mut events = Vec::new();
        for item in &response.output_items {
            match item {
                TalosOutputItem::Reasoning { text } => {
                    events.push(ExecutionEvent::text(
                        ctx,
                        EventAuthor::Agent,
                        response.branch,
                        ExecutionEventKind::Reasoning,
                        text.clone(),
                        true,
                    ));
                }
                TalosOutputItem::ExecutionSummary { text } => {
                    events.push(ExecutionEvent::text(
                        ctx,
                        EventAuthor::Agent,
                        response.branch,
                        ExecutionEventKind::Reasoning,
                        text.clone(),
                        true,
                    ));
                }
                TalosOutputItem::ToolResult {
                    call_id,
                    tool_name,
                    content,
                    is_error,
                } => {
                    let payload = serde_json::to_value(content).unwrap_or_else(|_| {
                        serde_json::json!({
                            "content": content.iter().map(|entry| match entry {
                                TalosContentItem::Text { text } => serde_json::json!({"text": text}),
                                TalosContentItem::Json { value } => value.clone(),
                            }).collect::<Vec<_>>()
                        })
                    });
                    events.push(ExecutionEvent::tool_result(
                        ctx,
                        response.branch,
                        call_id.clone(),
                        tool_name.clone(),
                        payload,
                        *is_error,
                    ));
                }
                TalosOutputItem::OutputText { .. } | TalosOutputItem::ToolCall { .. } => {}
            }
        }

        Ok(ResponseProcessorOutput {
            disposition: ResponseDisposition::Continue,
            events,
        })
    }
}

pub struct StateDeltaResponseProcessor;

#[async_trait]
impl ResponseProcessor for StateDeltaResponseProcessor {
    fn kind(&self) -> ResponseProcessorKind {
        ResponseProcessorKind::StateDelta
    }

    async fn run(
        &self,
        ctx: &mut InvocationContext,
        response: &mut LlmResponseEnvelope,
    ) -> TalosResult<ResponseProcessorOutput> {
        for delta in &response.actions.state_delta {
            match delta.namespace {
                crate::StateNamespace::Session => {
                    ctx.set_session_state(delta.key.clone(), delta.value.clone());
                }
                crate::StateNamespace::App => {
                    ctx.state.insert(delta.key.clone(), delta.value.clone());
                }
                crate::StateNamespace::Temp => {
                    ctx.set_request_state(delta.key.clone(), delta.value.clone());
                }
                crate::StateNamespace::User | crate::StateNamespace::Workspace => {}
            }
        }
        Ok(ResponseProcessorOutput::default())
    }
}

#[derive(Default)]
pub struct RequestProcessorPipeline {
    processors: Vec<Arc<dyn RequestProcessor>>,
}

impl RequestProcessorPipeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn for_profile(
        profile: FlowProfile,
        planner: Arc<dyn Planner>,
        agent_name: impl Into<String>,
        agent_description: impl Into<String>,
        allow_transfer: bool,
    ) -> Self {
        let agent_name = agent_name.into();
        let agent_description = agent_description.into();
        let mut pipeline = Self::new()
            .with_processor(Arc::new(BasicRequestProcessor))
            .with_processor(Arc::new(AuthRequestProcessor))
            .with_processor(Arc::new(RequestConfirmationRequestProcessor))
            .with_processor(Arc::new(DynamicToolReplayRequestProcessor))
            .with_processor(Arc::new(InstructionsRequestProcessor))
            .with_processor(Arc::new(IdentityRequestProcessor::new(
                agent_name,
                agent_description,
            )))
            .with_processor(Arc::new(CompactionRequestProcessor))
            .with_processor(Arc::new(ContentsRequestProcessor))
            .with_processor(Arc::new(ContextCacheRequestProcessor))
            .with_processor(Arc::new(InteractionsRequestProcessor));
        if matches!(
            profile,
            FlowProfile::DirectAnswer | FlowProfile::ModelThinking | FlowProfile::ReactiveLoop
        ) {
            pipeline = pipeline.with_processor(Arc::new(PlanningRequestProcessor::new(planner)));
        }
        pipeline = pipeline
            .with_processor(Arc::new(ToolContextRequestProcessor))
            .with_processor(Arc::new(OutputSchemaRequestProcessor));
        if allow_transfer {
            pipeline = pipeline.with_processor(Arc::new(AgentTransferRequestProcessor));
        }
        pipeline
    }

    pub fn with_processor(mut self, processor: Arc<dyn RequestProcessor>) -> Self {
        self.processors.push(processor);
        self
    }

    pub async fn run(
        &self,
        ctx: &mut InvocationContext,
        request: &mut LlmRequestEnvelope,
    ) -> TalosResult<Vec<ExecutionEvent>> {
        let mut events = Vec::new();
        for processor in &self.processors {
            let output = processor.run(ctx, request).await?;
            for event in &output.events {
                stage_request_event(ctx, event);
            }
            events.extend(output.events);
        }
        Ok(events)
    }
}

#[derive(Default)]
pub struct ResponseProcessorPipeline {
    processors: Vec<Arc<dyn ResponseProcessor>>,
}

impl ResponseProcessorPipeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn for_profile(profile: FlowProfile, planner: Arc<dyn Planner>) -> Self {
        let mut pipeline = Self::new();
        if matches!(profile, FlowProfile::ReactiveLoop) {
            pipeline = pipeline.with_processor(Arc::new(PlanningResponseProcessor::new(planner)));
        }
        pipeline
            .with_processor(Arc::new(ToolCallExtractionProcessor))
            .with_processor(Arc::new(FinalAnswerExtractionProcessor))
            .with_processor(Arc::new(StateDeltaResponseProcessor))
            .with_processor(Arc::new(EventProjectionProcessor))
    }

    pub fn with_processor(mut self, processor: Arc<dyn ResponseProcessor>) -> Self {
        self.processors.push(processor);
        self
    }

    pub async fn run(
        &self,
        ctx: &mut InvocationContext,
        response: &mut LlmResponseEnvelope,
    ) -> TalosResult<ResponseProcessorOutput> {
        let mut disposition = ResponseDisposition::Continue;
        let mut events = Vec::new();
        for processor in &self.processors {
            let current = processor.run(ctx, response).await?;
            if matches!(current.disposition, ResponseDisposition::Final) {
                disposition = ResponseDisposition::Final;
            }
            events.extend(current.events);
        }
        Ok(ResponseProcessorOutput {
            disposition,
            events,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BuiltInPlanner, ContextStateBags, ExecutionBranch, InvocationLimits, PlanReActPlanner,
        PlannerKind, PlanningResponseEffect, TurnPhase,
    };
    use serde_json::json;
    use std::collections::HashMap;

    struct BranchingPlanner;

    impl Planner for BranchingPlanner {
        fn mode(&self) -> crate::PlannerKind {
            PlannerKind::PlanReAct
        }

        fn process_response_effect(
            &self,
            _ctx: &InvocationContext,
            response: &LlmResponseEnvelope,
        ) -> PlanningResponseEffect {
            PlanningResponseEffect {
                normalized_text: Some(response.output_text.trim().to_string()),
                segments: Vec::new(),
                output_text_override: None,
                clear_output_text: false,
                state_delta: vec![crate::StateDelta {
                    namespace: crate::StateNamespace::Temp,
                    key: "plan_seen".to_string(),
                    value: json!(true),
                }],
                branch_override: Some(ExecutionBranch::DirectAnswer),
                has_final_answer: false,
            }
        }
    }

    fn mock_ctx() -> InvocationContext {
        InvocationContext {
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            agent_id: "a1".to_string(),
            workspace_id: "w1".to_string(),
            input: "hello".to_string(),
            model: "m1".to_string(),
            phase: TurnPhase::Execution,
            planning_instruction: None,
            transfer_to: None,
            execution_state: crate::InvocationExecutionState::default(),
            session: crate::InvocationSession {
                session_id: "s1".to_string(),
                ..crate::InvocationSession::default()
            },
            state: HashMap::new(),
            state_bags: ContextStateBags::default(),
            limits: InvocationLimits::default(),
            events: Vec::new(),
            plugin_manager: crate::default_plugin_manager(),
            session_service: None,
            stream_tx: None,
        }
    }

    #[tokio::test]
    async fn test_planning_response_processor_emits_typed_branch_override_event() {
        let planner: Arc<dyn Planner> = Arc::new(BranchingPlanner);
        let processor = PlanningResponseProcessor::new(planner);
        let mut ctx = mock_ctx();
        let mut response = LlmResponseEnvelope {
            output_text: "  answer ".to_string(),
            output_items: Vec::new(),
            branch: ExecutionBranch::ReactiveLoop,
            actions: EventActions::default(),
            allow_final_projection: true,
        };
        let output = processor.run(&mut ctx, &mut response).await.unwrap();
        let events = output.events;

        assert_eq!(output.disposition, ResponseDisposition::Continue);
        assert_eq!(
            ctx.execution_state.planning_response.as_deref(),
            Some("answer")
        );
        assert!(!ctx
            .state_bags
            .request_state
            .contains_key("execution_branch"));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].branch, ExecutionBranch::DirectAnswer);
        assert_eq!(
            events[0].actions.branch_override,
            Some(ExecutionBranch::DirectAnswer)
        );
        assert_eq!(events[0].actions.state_delta.len(), 1);
    }

    #[tokio::test]
    async fn test_planning_request_processor_applies_built_in_native_thinking() {
        let planner: Arc<dyn Planner> = Arc::new(BuiltInPlanner::new());
        let processor = PlanningRequestProcessor::new(planner);
        let mut ctx = mock_ctx();
        let mut request = LlmRequestEnvelope {
            branch: ExecutionBranch::ModelThinking,
            native_thinking_support: talos_llm_provider::NativeThinkingSupport::Supported,
            ..LlmRequestEnvelope::default()
        };

        processor.run(&mut ctx, &mut request).await.unwrap();

        assert!(request.native_thinking.is_some());
        assert!(request.planning_instruction.is_none());
    }

    #[tokio::test]
    async fn test_planning_response_processor_rewrites_plan_react_tagged_response() {
        let planner: Arc<dyn Planner> = Arc::new(PlanReActPlanner::new());
        let processor = PlanningResponseProcessor::new(planner);
        let mut ctx = mock_ctx();
        let mut response = LlmResponseEnvelope {
            output_text: "/*PLANNING*/ collect facts\n/*FINAL_ANSWER*/done".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            actions: EventActions::default(),
            output_items: Vec::new(),
            allow_final_projection: true,
        };

        let output = processor.run(&mut ctx, &mut response).await.unwrap();

        assert_eq!(output.disposition, ResponseDisposition::Continue);
        assert_eq!(response.output_text, "done");
        assert!(ctx.execution_state.plan_react_has_final_answer);
        assert!(response
            .output_items
            .iter()
            .any(|item| matches!(item, TalosOutputItem::Reasoning { text } if text.contains("/*PLANNING*/"))));
    }

    #[tokio::test]
    async fn test_final_answer_extraction_emits_final_event_and_marks_final() {
        let mut ctx = mock_ctx();
        let mut response = LlmResponseEnvelope {
            output_text: "done".to_string(),
            output_items: vec![TalosOutputItem::OutputText {
                text: "done".to_string(),
            }],
            branch: ExecutionBranch::DirectAnswer,
            actions: EventActions::default(),
            allow_final_projection: true,
        };
        let output = FinalAnswerExtractionProcessor
            .run(&mut ctx, &mut response)
            .await
            .unwrap();
        let events = output.events;

        assert_eq!(output.disposition, ResponseDisposition::Final);
        assert_eq!(events.len(), 1);
        assert!(events[0].is_final_response());
        assert_eq!(events[0].branch, ExecutionBranch::DirectAnswer);
    }

    #[tokio::test]
    async fn test_tool_call_extraction_emits_tool_call_events() {
        let mut ctx = mock_ctx();
        let mut response = LlmResponseEnvelope {
            output_text: String::new(),
            output_items: vec![TalosOutputItem::ToolCall {
                call_id: "call-1".to_string(),
                name: "read_file".to_string(),
                arguments: json!({"path": "README.md"}),
            }],
            branch: ExecutionBranch::ReactiveLoop,
            actions: EventActions::default(),
            allow_final_projection: true,
        };
        let output = ToolCallExtractionProcessor
            .run(&mut ctx, &mut response)
            .await
            .unwrap();
        let events = output.events;

        assert_eq!(output.disposition, ResponseDisposition::Continue);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].kind, ExecutionEventKind::ToolCall));
        assert_eq!(events[0].branch, ExecutionBranch::ReactiveLoop);
    }

    #[tokio::test]
    async fn test_tool_context_processor_disables_tools_for_direct_answer() {
        let mut ctx = mock_ctx();
        let mut request = LlmRequestEnvelope {
            tool_definitions: vec![talos_llm_provider::ToolDefinition {
                name: "read_file".to_string(),
                description: "read file".to_string(),
                parameters: Some(json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    }
                })),
            }],
            parallel_tool_calls: true,
            contents_mode: ContentsMode::FullSession,
            branch: ExecutionBranch::DirectAnswer,
            ..LlmRequestEnvelope::default()
        };
        ToolContextRequestProcessor
            .run(&mut ctx, &mut request)
            .await
            .unwrap();

        assert!(request.tool_definitions.is_empty());
        assert!(!request.parallel_tool_calls);
    }

    #[tokio::test]
    async fn test_agent_transfer_processor_emits_transfer_event_and_instruction() {
        let mut ctx = mock_ctx();
        ctx.transfer_to = Some("agent-b".to_string());
        let mut request = LlmRequestEnvelope {
            branch: ExecutionBranch::ReactiveLoop,
            ..LlmRequestEnvelope::default()
        };
        let output = AgentTransferRequestProcessor
            .run(&mut ctx, &mut request)
            .await
            .unwrap();

        let events = output.events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].actions.transfer_to.as_deref(), Some("agent-b"));
        assert!(request
            .prompt_prefix_messages
            .iter()
            .any(|message| message.content.contains("agent-b")));
    }

    #[tokio::test]
    async fn test_compaction_processor_emits_typed_summary_event() {
        let mut ctx = mock_ctx();
        ctx.limits.context_compaction_max_bytes = 32;
        ctx.session.events = vec![
            crate::SessionEvent {
                id: "evt-old-1".to_string(),
                session_id: "s1".to_string(),
                invocation_id: "old-1".to_string(),
                branch: ExecutionBranch::ReactiveLoop,
                author: EventAuthor::User,
                kind: crate::SessionEventKind::UserMessage,
                payload: crate::SessionEventPayload::Message {
                    message: ChatMessage::user("first user message that is long enough"),
                },
                actions: crate::EventActions::default(),
                partial: false,
                timestamp: chrono::Utc::now(),
            },
            crate::SessionEvent {
                id: "evt-old-1-assistant".to_string(),
                session_id: "s1".to_string(),
                invocation_id: "old-1".to_string(),
                branch: ExecutionBranch::ReactiveLoop,
                author: EventAuthor::Agent,
                kind: crate::SessionEventKind::AgentMessage,
                payload: crate::SessionEventPayload::Message {
                    message: ChatMessage::assistant("first assistant reply that is long enough"),
                },
                actions: crate::EventActions::default(),
                partial: false,
                timestamp: chrono::Utc::now(),
            },
            crate::SessionEvent {
                id: "evt-old-2".to_string(),
                session_id: "s1".to_string(),
                invocation_id: "old-2".to_string(),
                branch: ExecutionBranch::ReactiveLoop,
                author: EventAuthor::User,
                kind: crate::SessionEventKind::UserMessage,
                payload: crate::SessionEventPayload::Message {
                    message: ChatMessage::user("second user message that is long enough"),
                },
                actions: crate::EventActions::default(),
                partial: false,
                timestamp: chrono::Utc::now(),
            },
            crate::SessionEvent {
                id: "evt-old-2-assistant".to_string(),
                session_id: "s1".to_string(),
                invocation_id: "old-2".to_string(),
                branch: ExecutionBranch::ReactiveLoop,
                author: EventAuthor::Agent,
                kind: crate::SessionEventKind::AgentMessage,
                payload: crate::SessionEventPayload::Message {
                    message: ChatMessage::assistant("second assistant reply that is long enough"),
                },
                actions: crate::EventActions::default(),
                partial: false,
                timestamp: chrono::Utc::now(),
            },
            crate::SessionEvent {
                id: "evt-old-3".to_string(),
                session_id: "s1".to_string(),
                invocation_id: "old-3".to_string(),
                branch: ExecutionBranch::ReactiveLoop,
                author: EventAuthor::User,
                kind: crate::SessionEventKind::UserMessage,
                payload: crate::SessionEventPayload::Message {
                    message: ChatMessage::user("third user message that is long enough"),
                },
                actions: crate::EventActions::default(),
                partial: false,
                timestamp: chrono::Utc::now(),
            },
        ];
        let mut request = LlmRequestEnvelope {
            branch: ExecutionBranch::ReactiveLoop,
            contents_mode: ContentsMode::FullSession,
            ..LlmRequestEnvelope::default()
        };

        let output = CompactionRequestProcessor
            .run(&mut ctx, &mut request)
            .await
            .unwrap();

        assert_eq!(output.events.len(), 1);
        let compaction = output.events[0]
            .actions
            .compaction
            .as_ref()
            .expect("expected compaction actions");
        assert_eq!(compaction.start_event_id, "evt-old-1");
        assert_eq!(compaction.end_event_id, "evt-old-2-assistant");
        match &output.events[0].payload {
            EventPayload::CompactionSummary { payload } => {
                assert!(payload.summary.contains("Compacted previous context"));
            }
            other => panic!("expected compaction summary payload, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_compaction_processor_compacts_whole_invocations_only() {
        let mut ctx = mock_ctx();
        ctx.limits.context_compaction_max_bytes = 40;
        ctx.session.events = vec![
            crate::SessionEvent {
                id: "evt-old-1-user".to_string(),
                session_id: "s1".to_string(),
                invocation_id: "old-1".to_string(),
                branch: ExecutionBranch::ReactiveLoop,
                author: EventAuthor::User,
                kind: crate::SessionEventKind::UserMessage,
                payload: crate::SessionEventPayload::Message {
                    message: ChatMessage::user("first invocation user message that is long enough"),
                },
                actions: crate::EventActions::default(),
                partial: false,
                timestamp: chrono::Utc::now(),
            },
            crate::SessionEvent {
                id: "evt-old-1-agent".to_string(),
                session_id: "s1".to_string(),
                invocation_id: "old-1".to_string(),
                branch: ExecutionBranch::ReactiveLoop,
                author: EventAuthor::Agent,
                kind: crate::SessionEventKind::AgentMessage,
                payload: crate::SessionEventPayload::Message {
                    message: ChatMessage::assistant(
                        "first invocation assistant message that is long enough",
                    ),
                },
                actions: crate::EventActions::default(),
                partial: false,
                timestamp: chrono::Utc::now(),
            },
            crate::SessionEvent {
                id: "evt-old-2-user".to_string(),
                session_id: "s1".to_string(),
                invocation_id: "old-2".to_string(),
                branch: ExecutionBranch::ReactiveLoop,
                author: EventAuthor::User,
                kind: crate::SessionEventKind::UserMessage,
                payload: crate::SessionEventPayload::Message {
                    message: ChatMessage::user("second invocation user message that is also long"),
                },
                actions: crate::EventActions::default(),
                partial: false,
                timestamp: chrono::Utc::now(),
            },
            crate::SessionEvent {
                id: "evt-old-2-agent".to_string(),
                session_id: "s1".to_string(),
                invocation_id: "old-2".to_string(),
                branch: ExecutionBranch::ReactiveLoop,
                author: EventAuthor::Agent,
                kind: crate::SessionEventKind::AgentMessage,
                payload: crate::SessionEventPayload::Message {
                    message: ChatMessage::assistant("second invocation assistant message"),
                },
                actions: crate::EventActions::default(),
                partial: false,
                timestamp: chrono::Utc::now(),
            },
            crate::SessionEvent {
                id: "evt-old-3-user".to_string(),
                session_id: "s1".to_string(),
                invocation_id: "old-3".to_string(),
                branch: ExecutionBranch::ReactiveLoop,
                author: EventAuthor::User,
                kind: crate::SessionEventKind::UserMessage,
                payload: crate::SessionEventPayload::Message {
                    message: ChatMessage::user("third invocation user message"),
                },
                actions: crate::EventActions::default(),
                partial: false,
                timestamp: chrono::Utc::now(),
            },
        ];
        let mut request = LlmRequestEnvelope {
            branch: ExecutionBranch::ReactiveLoop,
            contents_mode: ContentsMode::FullSession,
            ..LlmRequestEnvelope::default()
        };

        let output = CompactionRequestProcessor
            .run(&mut ctx, &mut request)
            .await
            .unwrap();

        let compaction = output.events[0]
            .actions
            .compaction
            .as_ref()
            .expect("expected compaction actions");
        assert_eq!(compaction.start_event_id, "evt-old-1-user");
        assert_eq!(compaction.end_event_id, "evt-old-2-agent");
    }

    #[tokio::test]
    async fn test_compaction_processor_skips_current_invocation_events() {
        let mut ctx = mock_ctx();
        ctx.invocation_id = "current".to_string();
        ctx.limits.context_compaction_max_bytes = 32;
        ctx.session.events = vec![
            crate::SessionEvent {
                id: "evt-history".to_string(),
                session_id: "s1".to_string(),
                invocation_id: "history".to_string(),
                branch: ExecutionBranch::ReactiveLoop,
                author: EventAuthor::User,
                kind: crate::SessionEventKind::UserMessage,
                payload: crate::SessionEventPayload::Message {
                    message: ChatMessage::user("short"),
                },
                actions: crate::EventActions::default(),
                partial: false,
                timestamp: chrono::Utc::now(),
            },
            crate::SessionEvent {
                id: "evt-current-1".to_string(),
                session_id: "s1".to_string(),
                invocation_id: "current".to_string(),
                branch: ExecutionBranch::ReactiveLoop,
                author: EventAuthor::User,
                kind: crate::SessionEventKind::UserMessage,
                payload: crate::SessionEventPayload::Message {
                    message: ChatMessage::user("current invocation message that is very long"),
                },
                actions: crate::EventActions::default(),
                partial: false,
                timestamp: chrono::Utc::now(),
            },
            crate::SessionEvent {
                id: "evt-current-2".to_string(),
                session_id: "s1".to_string(),
                invocation_id: "current".to_string(),
                branch: ExecutionBranch::ReactiveLoop,
                author: EventAuthor::Agent,
                kind: crate::SessionEventKind::AgentMessage,
                payload: crate::SessionEventPayload::Message {
                    message: ChatMessage::assistant("current invocation reply that is very long"),
                },
                actions: crate::EventActions::default(),
                partial: false,
                timestamp: chrono::Utc::now(),
            },
        ];
        let mut request = LlmRequestEnvelope {
            branch: ExecutionBranch::ReactiveLoop,
            contents_mode: ContentsMode::FullSession,
            ..LlmRequestEnvelope::default()
        };

        let output = CompactionRequestProcessor
            .run(&mut ctx, &mut request)
            .await
            .unwrap();

        assert!(output.events.is_empty());
    }

    fn make_confirmation_event(
        call_id: &str,
        tool_name: &str,
        ts_offset_ms: i64,
    ) -> crate::SessionEvent {
        let timestamp = chrono::Utc::now() + chrono::Duration::milliseconds(ts_offset_ms);
        crate::SessionEvent {
            id: format!("conf-{}", call_id),
            session_id: "s1".to_string(),
            invocation_id: "i0".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: crate::EventAuthor::System,
            kind: crate::SessionEventKind::SystemMessage,
            payload: crate::SessionEventPayload::Confirmation {
                payload: crate::SessionConfirmationPayload {
                    call_id: call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    prompt: "Need approval.".to_string(),
                    arguments: json!({"cmd": "echo hi"}),
                },
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp,
        }
    }

    fn make_approval_event(
        call_id: &str,
        tool_name: &str,
        approved: bool,
        ts_offset_ms: i64,
    ) -> crate::SessionEvent {
        let timestamp = chrono::Utc::now() + chrono::Duration::milliseconds(ts_offset_ms);
        crate::SessionEvent {
            id: format!("approval-{}", call_id),
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            branch: ExecutionBranch::ReactiveLoop,
            author: crate::EventAuthor::System,
            kind: crate::SessionEventKind::SystemMessage,
            payload: crate::SessionEventPayload::ApprovalResponse {
                payload: crate::SessionApprovalResponsePayload {
                    call_id: call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    approved,
                    reason: if approved {
                        None
                    } else {
                        Some("user denied".to_string())
                    },
                },
            },
            actions: crate::EventActions::default(),
            partial: false,
            timestamp,
        }
    }

    #[tokio::test]
    async fn test_confirmation_processor_injects_replay_when_approved() {
        let mut ctx = mock_ctx();
        ctx.session.events = vec![
            make_confirmation_event("call-1", "exec_command", -100),
            make_approval_event("call-1", "exec_command", true, 0),
        ];
        let mut request = LlmRequestEnvelope {
            branch: ExecutionBranch::ReactiveLoop,
            ..LlmRequestEnvelope::default()
        };

        RequestConfirmationRequestProcessor
            .run(&mut ctx, &mut request)
            .await
            .unwrap();

        assert_eq!(request.replay_tool_invocations.len(), 1);
        assert_eq!(request.replay_tool_invocations[0].call_id, "call-1");
        assert_eq!(request.replay_tool_invocations[0].tool_name, "exec_command");
        assert!(request.prompt_prefix_messages.is_empty());
    }

    #[tokio::test]
    async fn test_confirmation_processor_still_blocks_when_no_response() {
        let mut ctx = mock_ctx();
        ctx.session.events = vec![make_confirmation_event("call-2", "exec_command", -100)];
        let mut request = LlmRequestEnvelope {
            branch: ExecutionBranch::ReactiveLoop,
            ..LlmRequestEnvelope::default()
        };

        RequestConfirmationRequestProcessor
            .run(&mut ctx, &mut request)
            .await
            .unwrap();

        assert!(request.replay_tool_invocations.is_empty());
        assert!(request
            .prompt_prefix_messages
            .iter()
            .any(|m| { m.content.contains("call-2") && m.content.contains("pending") }));
    }

    #[tokio::test]
    async fn test_confirmation_processor_handles_rejection() {
        let mut ctx = mock_ctx();
        ctx.session.events = vec![
            make_confirmation_event("call-3", "exec_command", -100),
            make_approval_event("call-3", "exec_command", false, 0),
        ];
        let mut request = LlmRequestEnvelope {
            branch: ExecutionBranch::ReactiveLoop,
            ..LlmRequestEnvelope::default()
        };

        RequestConfirmationRequestProcessor
            .run(&mut ctx, &mut request)
            .await
            .unwrap();

        assert!(request.replay_tool_invocations.is_empty());
        assert!(request
            .prompt_prefix_messages
            .iter()
            .any(|m| { m.content.contains("rejected") || m.content.contains("denied") }));
    }
}
