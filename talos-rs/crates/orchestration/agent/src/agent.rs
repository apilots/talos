use async_trait::async_trait;
use chrono::Utc;
use dashmap::DashMap;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task::AbortHandle;
use uuid::Uuid;

mod kernel;
mod mock;
mod request;
mod session;
#[cfg(test)]
mod tests;

pub use mock::MockAgent;

use crate::{
    flush_manager::MemoryFlushManager, AgentOrchestrator, OrchestratorExecution,
    OrchestratorRequest, SessionTitleUpdate,
};
use talos_agent_kernel::{
    default_plugin_manager, run_general_turn_with_resume, ContextStateBags,
    DynamicToolResultResume, GeneralTurnOutcome, GeneralTurnResult, GeneralTurnResume,
    InvocationContext, InvocationExecutionState, InvocationLimits, InvocationSession,
    KernelAssistantTextPhase, KernelEvent, KernelEventData, KernelEventLabel, KernelEventType,
    KernelStreamEvent, KernelStreamEventData, KernelStreamEventType, KernelStreamItemKey,
    ResumeToolCall, StopReason as KernelStopReason, TimelineBuilder, ToolProgressKind,
    ToolStreamKey, UserInputEventData,
};
use talos_config::{AgentKernelConfig, PlannerMode};
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{ChatMessage, ChatRequest, LlmProvider};
use talos_memory::{Memory, MemoryStore, SessionTranscriptBranch, SessionTranscriptPayload};
use talos_tools::ToolRegistry;
use talos_types::gateway::GatewayResumeContext;
use talos_types::normalize_session_title;
use talos_types::talos::{
    StopReason as TalosStopReason, TalosContentItem, TalosOutputItem, TalosTimelineContent,
    TalosTimelineDelta, TalosTimelineEvent, TalosTimelineEventPayload, TalosTimelineItemKind,
    TalosTimelineItemStatus, TalosTimelinePhase, TalosToolProgressKind, TalosUsage,
    TurnExecutionMetrics,
};
use tracing::warn;

const SESSION_TITLE_PROMPT: &str = r#"Generate a concise session title for the conversation.

Rules:
- Return only the title text.
- One line only.
- No markdown, no quotes, no trailing punctuation.
- Prefer 3-8 words.
- Focus on the core task topic.
"#;

#[derive(Clone)]
pub(crate) enum ActiveTurnHandle {
    Reserved,
    Running { execution_task: AbortHandle },
}

pub(crate) enum QueuedTurn {
    Unary {
        request: OrchestratorRequest,
        response_tx: oneshot::Sender<TalosResult<OrchestratorExecution>>,
    },
    Stream {
        request: OrchestratorRequest,
        event_tx: mpsc::Sender<TalosTimelineEvent>,
    },
}

#[derive(Clone)]
pub struct AgentHarness {
    pub id: String,
    pub name: String,
    pub description: String,
    pub llm_provider: Arc<dyn LlmProvider>,
    pub model: String,
    pub memory_store: Arc<MemoryStore>,
    pub tool_registry: Arc<ToolRegistry>,
    kernel_config: AgentKernelConfig,
    active_turns: Arc<DashMap<String, ActiveTurnHandle>>,
    completed_title_updates: Arc<DashMap<String, SessionTitleUpdate>>,
    session_backlogs: Arc<Mutex<HashMap<String, VecDeque<QueuedTurn>>>>,
    memory_flush_manager: Arc<MemoryFlushManager>,
}

impl AgentHarness {
    pub fn new(
        id: String,
        name: String,
        description: String,
        llm_provider: Arc<dyn LlmProvider>,
        model: String,
        tool_registry: Arc<ToolRegistry>,
        kernel_config: AgentKernelConfig,
    ) -> Self {
        let kernel_config = Self::normalized_kernel_config(kernel_config);
        let memory_store = Arc::new(Self::build_memory_store_from_kernel_config(
            &kernel_config,
            llm_provider.clone(),
        ));

        Self {
            id,
            name,
            description,
            llm_provider,
            model,
            memory_store,
            tool_registry,
            kernel_config,
            active_turns: Arc::new(DashMap::new()),
            completed_title_updates: Arc::new(DashMap::new()),
            session_backlogs: Arc::new(Mutex::new(HashMap::new())),
            memory_flush_manager: Arc::new(MemoryFlushManager::new()),
        }
    }

    /// Add a tool to this agent's registry
    pub fn add_tool(&self, tool: Arc<dyn talos_tools::Tool>) {
        self.tool_registry.register(tool);
    }

    /// Set the tool registry for this agent (allows external tool injection)
    pub fn set_tool_registry(&mut self, tool_registry: Arc<ToolRegistry>) {
        self.tool_registry = tool_registry;
    }

    pub fn set_default_planner_mode(&mut self, planner_mode: &str) -> TalosResult<()> {
        let mode = PlannerMode::parse(planner_mode.trim())?;
        self.kernel_config.planner_mode = mode;
        Ok(())
    }

    /// Update kernel configuration with a new config object.
    pub fn update_kernel_config(&mut self, config: AgentKernelConfig) {
        self.kernel_config = Self::normalized_kernel_config(config);
        self.rebuild_memory_store();
    }

    /// Generate a system prompt that includes tool information
    pub fn generate_system_prompt(&self) -> String {
        let available_tools = self.tool_registry.list_tool_names();
        format!(
            r#"You are {}, {}

You have access to the following tools:
{}

Use native function calling when a tool is needed.
After receiving tool outputs, continue reasoning and provide the final response."#,
            self.name,
            self.description,
            available_tools.join(", ")
        )
    }

    fn build_invocation_context(
        &self,
        agent_request: &crate::service::AgentRequest,
        input: String,
        invocation_id: String,
        stream_tx: Option<mpsc::Sender<KernelStreamEvent>>,
        kernel_config: &AgentKernelConfig,
    ) -> InvocationContext {
        let session_id = agent_request.session.context.session_id.clone();
        let message_id = agent_request.session.context.message_id.clone();
        let workspace_id = agent_request
            .session
            .context
            .workspace_id
            .clone()
            .unwrap_or_default();

        InvocationContext {
            session_id: session_id.clone(),
            message_id,
            invocation_id,
            agent_id: self.id.clone(),
            workspace_id,
            project_root: agent_request.session.context.project_root.clone(),
            input,
            selected_model_info: self.resolve_selected_model_info(&agent_request.session),
            phase: agent_request.phase.clone(),
            planning_instruction: agent_request.planning_instruction.clone(),
            transfer_to: agent_request.transfer_to.clone(),
            execution_state: InvocationExecutionState::default(),
            session: InvocationSession {
                session_id: session_id.clone(),
                ..InvocationSession::default()
            },
            session_environment: agent_request.session.context.environment.clone(),
            state: HashMap::new(),
            state_bags: ContextStateBags::default(),
            limits: InvocationLimits::from(kernel_config),
            events: vec![KernelEvent::new(
                KernelEventType::UserInput,
                KernelEventLabel::UserInputReceived,
                KernelEventData::UserInput(UserInputEventData {
                    session_id,
                    agent_id: self.id.clone(),
                }),
            )],
            plugin_manager: default_plugin_manager(),
            transcript_service: None,
            stream_tx,
        }
    }

    async fn build_general_turn_resume(
        &self,
        memory_store: &MemoryStore,
        session_id: &str,
        resume_context: Option<GatewayResumeContext>,
    ) -> TalosResult<Option<GeneralTurnResume>> {
        let Some(tool_call) = resume_context.and_then(|context| context.tool_call) else {
            return Ok(None);
        };

        let records = memory_store
            .load_transcript_records(session_id, None, SessionTranscriptBranch::ReactiveLoop)
            .await?;
        let mut request = None;
        let mut response = None;
        for record in records {
            match record.payload {
                SessionTranscriptPayload::DynamicToolRequest { payload }
                    if payload.call_id.as_str() == tool_call.call_id.as_str()
                        && payload.tool_name.as_str() == tool_call.tool_name.as_str() =>
                {
                    if request
                        .as_ref()
                        .is_none_or(|(timestamp, _)| record.header.timestamp > *timestamp)
                    {
                        request = Some((record.header.timestamp, payload));
                    }
                }
                SessionTranscriptPayload::DynamicToolResponse { payload }
                    if payload.call_id.as_str() == tool_call.call_id.as_str()
                        && payload.tool_name.as_str() == tool_call.tool_name.as_str() =>
                {
                    if response
                        .as_ref()
                        .is_none_or(|(timestamp, _)| record.header.timestamp > *timestamp)
                    {
                        response = Some((record.header.timestamp, payload));
                    }
                }
                _ => {}
            }
        }
        let response = response.map(|(_, payload)| payload).ok_or_else(|| {
            TalosError::ToolError(format!(
                "dynamic tool call `{}` has no submitted result for session `{}`",
                tool_call.call_id, session_id
            ))
        })?;
        let original_arguments = request
            .map(|(_, payload)| payload.arguments)
            .unwrap_or_else(|| serde_json::Value::Null.into());

        Ok(Some(GeneralTurnResume::DynamicToolResult(
            DynamicToolResultResume {
                call_id: tool_call.call_id.clone(),
                tool_name: tool_call.tool_name.clone(),
                output: response.output,
                success: response.success,
                original_call: Some(ResumeToolCall {
                    item_id: Some(tool_call.item_id),
                    call_id: tool_call.call_id,
                    tool_name: tool_call.tool_name,
                    arguments: original_arguments,
                }),
                replay_dynamic_tool_results: HashMap::new(),
            },
        )))
    }

    async fn generate_session_title_update(
        &self,
        session: &talos_types::TalosSession,
        first_user_text: &str,
        first_assistant_text: &str,
    ) -> Option<SessionTitleUpdate> {
        if session.has_real_title()
            || session.state.message_count != 0
            || first_user_text.trim().is_empty()
            || first_assistant_text.trim().is_empty()
        {
            return None;
        }

        let request = ChatRequest::builder()
            .model(session.config.llm.model.clone())
            .messages(vec![
                ChatMessage::system(SESSION_TITLE_PROMPT),
                ChatMessage::user(format!(
                    "First user message:\n{}\n\nFirst assistant response:\n{}",
                    first_user_text.trim(),
                    first_assistant_text.trim()
                )),
            ])
            .build();

        match self.llm_provider.chat(request).await {
            Ok(response) => {
                normalize_session_title(&response.content).map(|title| SessionTitleUpdate { title })
            }
            Err(error) => {
                warn!(
                    session_id = %session.context.session_id,
                    ?error,
                    "failed to generate session title"
                );
                None
            }
        }
    }

    fn title_update_key(session_id: &str, turn_id: &str) -> String {
        format!("{session_id}:{turn_id}")
    }

    async fn reserve_or_enqueue_unary_turn(
        &self,
        request: OrchestratorRequest,
    ) -> TalosResult<Option<oneshot::Receiver<TalosResult<OrchestratorExecution>>>> {
        let session_id = request.session.context.session_id.clone();
        if self.has_queued_turns(&session_id).await {
            let (response_tx, response_rx) = oneshot::channel();
            self.enqueue_turn(
                &session_id,
                QueuedTurn::Unary {
                    request,
                    response_tx,
                },
            )
            .await;
            return Ok(Some(response_rx));
        }

        match self.try_reserve_active_turn(&session_id) {
            Ok(()) => Ok(None),
            Err(_) => {
                let (response_tx, response_rx) = oneshot::channel();
                self.enqueue_turn(
                    &session_id,
                    QueuedTurn::Unary {
                        request,
                        response_tx,
                    },
                )
                .await;
                Ok(Some(response_rx))
            }
        }
    }

    async fn reserve_or_enqueue_stream_turn(
        &self,
        request: OrchestratorRequest,
        event_tx: mpsc::Sender<TalosTimelineEvent>,
    ) -> TalosResult<bool> {
        let session_id = request.session.context.session_id.clone();
        if self.has_queued_turns(&session_id).await {
            self.enqueue_turn(&session_id, QueuedTurn::Stream { request, event_tx })
                .await;
            return Ok(false);
        }

        match self.try_reserve_active_turn(&session_id) {
            Ok(()) => Ok(true),
            Err(_) => {
                self.enqueue_turn(&session_id, QueuedTurn::Stream { request, event_tx })
                    .await;
                Ok(false)
            }
        }
    }

    async fn execute_reserved_run(
        &self,
        request: OrchestratorRequest,
    ) -> TalosResult<OrchestratorExecution> {
        let resume_context = request.resume_context.clone();
        let agent_request = self.to_agent_request(request);
        let input = Self::extract_text_input(&agent_request.input);
        let invocation_id = format!("inv-{}", Utc::now().timestamp_millis());
        let session_id = agent_request.session.context.session_id.clone();
        let (kernel_config, memory_store) = self
            .memory_store_for_session(&agent_request.session)
            .await?;
        let mut ctx = self.build_invocation_context(
            &agent_request,
            input.clone(),
            invocation_id,
            None,
            &kernel_config,
        );
        let resume = self
            .build_general_turn_resume(memory_store.as_ref(), &session_id, resume_context)
            .await?;
        let llm_provider = self.llm_provider.clone();
        let tool_registry = self.tool_registry.clone();
        let agent_name = self.name.clone();
        let agent_description = self.description.clone();
        let title_input = input.clone();
        let kernel_input = input;
        let run_handle = tokio::spawn({
            async move {
                run_general_turn_with_resume(
                    &mut ctx,
                    &agent_name,
                    &agent_description,
                    &kernel_input,
                    llm_provider,
                    tool_registry.clone(),
                    tool_registry,
                    memory_store,
                    &kernel_config,
                    resume,
                )
                .await
            }
        });
        let abort_handle = run_handle.abort_handle();
        if let Err(err) = self.activate_reserved_turn(&session_id, abort_handle) {
            run_handle.abort();
            self.finish_active_turn_and_resume(&session_id).await;
            return Err(err);
        }

        let result = match run_handle.await {
            Ok(Ok(result)) => {
                let (output_text, output_items, stop_reason, usage) =
                    execution_from_general_turn_result(&result);
                let reasoning = Self::collect_reasoning(&output_items);
                let session_title_update = self
                    .generate_session_title_update(
                        &agent_request.session,
                        &title_input,
                        &output_text,
                    )
                    .await;
                Ok(OrchestratorExecution {
                    output_text,
                    output_items,
                    reasoning,
                    stop_reason,
                    usage,
                    task_count: 1,
                    session_id: session_id.clone(),
                    session_title_update,
                })
            }
            Ok(Err(err)) => Err(err),
            Err(err) if err.is_cancelled() => Ok(OrchestratorExecution {
                output_text: String::new(),
                output_items: Vec::new(),
                reasoning: Vec::new(),
                stop_reason: TalosStopReason::Cancelled,
                usage: TalosUsage::default(),
                task_count: 0,
                session_id: session_id.clone(),
                session_title_update: None,
            }),
            Err(err) => Err(TalosError::InternalError(format!(
                "kernel task join failed: {}",
                err
            ))),
        };
        self.finish_active_turn_and_resume(&session_id).await;
        result
    }

    fn spawn_reserved_stream_turn(
        &self,
        request: OrchestratorRequest,
        tx: mpsc::Sender<TalosTimelineEvent>,
    ) {
        let this = self.clone();
        let stream_task = tokio::spawn(async move {
            let request_turn_id = request.turn_id.clone();
            let resume_context = request.resume_context.clone();
            let agent_request = this.to_agent_request(request);
            let input = Self::extract_text_input(&agent_request.input);
            let request_id = format!("req_{}", Uuid::new_v4().simple());
            let invocation_id = format!("inv-{}", Utc::now().timestamp_millis());
            let session_id = agent_request.session.context.session_id.clone();
            let message_id = agent_request.session.context.message_id.clone();
            let turn_id = request_turn_id.unwrap_or_else(|| request_id.clone());
            let title_turn_id = turn_id.clone();
            let mut timeline = TimelineBuilder::new(
                request_id.clone(),
                turn_id,
                message_id.clone(),
                session_id.clone(),
            );
            let mut timeline_state = KernelTimelineState::default();
            seed_resume_context(&mut timeline, &mut timeline_state, resume_context.clone());
            let (kernel_config, memory_store) =
                match this.memory_store_for_session(&agent_request.session).await {
                    Ok(value) => value,
                    Err(err) => {
                        let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                        this.finish_active_turn_and_resume(&session_id).await;
                        return;
                    }
                };
            let (kernel_tx, mut kernel_rx) = mpsc::channel(256);
            let mut kernel_ctx = this.build_invocation_context(
                &agent_request,
                input.clone(),
                invocation_id,
                Some(kernel_tx),
                &kernel_config,
            );
            let resume = match this
                .build_general_turn_resume(
                    memory_store.as_ref(),
                    &session_id,
                    resume_context.clone(),
                )
                .await
            {
                Ok(resume) => resume,
                Err(err) => {
                    let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                    this.finish_active_turn_and_resume(&session_id).await;
                    return;
                }
            };

            let _ = tx.send(timeline.start_turn()).await;
            let _ = tx
                .send(timeline.turn_metrics(TurnExecutionMetrics::default()))
                .await;
            if !input.trim().is_empty() {
                match emit_user_input_item(&mut timeline, &input) {
                    Ok(events) => {
                        for event in events {
                            let _ = tx.send(event).await;
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                        this.finish_active_turn_and_resume(&session_id).await;
                        return;
                    }
                }
            }

            let llm_provider = this.llm_provider.clone();
            let tool_registry = this.tool_registry.clone();
            let agent_name = this.name.clone();
            let agent_description = this.description.clone();
            let title_input = input.clone();
            let kernel_input = input;
            let run_handle = tokio::spawn(async move {
                run_general_turn_with_resume(
                    &mut kernel_ctx,
                    &agent_name,
                    &agent_description,
                    &kernel_input,
                    llm_provider,
                    tool_registry.clone(),
                    tool_registry,
                    memory_store,
                    &kernel_config,
                    resume,
                )
                .await
            });
            let abort_handle = run_handle.abort_handle();
            if let Err(err) = this.activate_reserved_turn(&session_id, abort_handle) {
                run_handle.abort();
                let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                this.finish_active_turn_and_resume(&session_id).await;
                return;
            }

            let mut stream_failed = false;
            while let Some(kernel_event) = kernel_rx.recv().await {
                match emit_kernel_timeline_events(&mut timeline, &mut timeline_state, kernel_event)
                {
                    Ok(events) => {
                        for event in events {
                            let _ = tx.send(event).await;
                        }
                    }
                    Err(err) => {
                        emit_terminal_item_closures(
                            &mut timeline,
                            &tx,
                            TalosTimelineItemStatus::Failed,
                        )
                        .await;
                        let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                        stream_failed = true;
                        run_handle.abort();
                        break;
                    }
                }
            }
            if stream_failed {
                this.finish_active_turn_and_resume(&session_id).await;
                return;
            }

            match run_handle.await {
                Ok(Ok(result)) => {
                    match emit_missing_tool_items_from_result(
                        &mut timeline,
                        &mut timeline_state,
                        &result.output_items,
                    ) {
                        Ok(events) => {
                            for event in events {
                                let _ = tx.send(event).await;
                            }
                        }
                        Err(err) => {
                            let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                            this.finish_active_turn_and_resume(&session_id).await;
                            return;
                        }
                    }
                    if matches!(result.outcome, GeneralTurnOutcome::Completed { .. }) {
                        emit_terminal_item_closures(
                            &mut timeline,
                            &tx,
                            TalosTimelineItemStatus::Completed,
                        )
                        .await;
                    }
                    let _ = tx
                        .send(timeline.turn_metrics(outcome_metrics(&result.outcome)))
                        .await;
                    let _ = tx
                        .send(timeline.complete_turn(
                            kernel::map_stop_reason(outcome_stop_reason(&result.outcome)),
                            outcome_usage(&result.outcome),
                            Some(outcome_metrics(&result.outcome)),
                        ))
                        .await;
                    this.finish_active_turn_and_resume(&session_id).await;
                    let (output_text, _, _, _) = execution_from_general_turn_result(&result);
                    if let Some(update) = this
                        .generate_session_title_update(
                            &agent_request.session,
                            &title_input,
                            &output_text,
                        )
                        .await
                    {
                        this.completed_title_updates
                            .insert(Self::title_update_key(&session_id, &title_turn_id), update);
                    }
                }
                Err(err) if err.is_cancelled() => {
                    emit_terminal_item_closures(
                        &mut timeline,
                        &tx,
                        TalosTimelineItemStatus::Cancelled,
                    )
                    .await;
                    let _ = tx
                        .send(timeline.complete_turn(
                            TalosStopReason::Cancelled,
                            TalosUsage::default(),
                            None,
                        ))
                        .await;
                    this.finish_active_turn_and_resume(&session_id).await;
                }
                Ok(Err(err)) => {
                    emit_terminal_item_closures(
                        &mut timeline,
                        &tx,
                        TalosTimelineItemStatus::Failed,
                    )
                    .await;
                    let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                    this.finish_active_turn_and_resume(&session_id).await;
                }
                Err(err) => {
                    emit_terminal_item_closures(
                        &mut timeline,
                        &tx,
                        TalosTimelineItemStatus::Failed,
                    )
                    .await;
                    let _ = tx
                        .send(
                            timeline.fail_turn(format!("kernel stream task join failed: {}", err)),
                        )
                        .await;
                    this.finish_active_turn_and_resume(&session_id).await;
                }
            }
        });
        drop(stream_task);
    }

    async fn finish_active_turn_and_resume(&self, session_id: &str) {
        if let Some(queued_turn) = self.dequeue_next_turn(session_id).await {
            self.active_turns
                .insert(session_id.to_string(), ActiveTurnHandle::Reserved);
            self.dispatch_queued_turn(queued_turn);
        } else {
            self.clear_active_turn(session_id);
        }
    }

    fn dispatch_queued_turn(&self, queued_turn: QueuedTurn) {
        match queued_turn {
            QueuedTurn::Unary {
                request,
                response_tx,
            } => {
                let this = self.clone();
                let task = tokio::spawn(async move {
                    let result = this.execute_reserved_run(request).await;
                    let _ = response_tx.send(result);
                });
                drop(task);
            }
            QueuedTurn::Stream { request, event_tx } => {
                self.spawn_reserved_stream_turn(request, event_tx);
            }
        }
    }
}

fn emit_user_input_item(
    timeline: &mut TimelineBuilder,
    input: &str,
) -> TalosResult<Vec<TalosTimelineEvent>> {
    let started = timeline.start_user_input(input.to_string());
    let item_id = item_id_from_started(&started)?;
    let completed = timeline.complete_item(&item_id, TalosTimelineItemStatus::Completed, None)?;
    Ok(vec![started, completed])
}

async fn emit_terminal_item_closures(
    timeline: &mut TimelineBuilder,
    tx: &mpsc::Sender<TalosTimelineEvent>,
    status: TalosTimelineItemStatus,
) {
    match timeline.close_active_items(status) {
        Ok(events) => {
            for event in events {
                let _ = tx.send(event).await;
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, "failed to close active timeline items");
        }
    }
}

#[derive(Default)]
struct KernelTimelineState {
    reasoning_item_id: Option<String>,
    final_answer_item_id: Option<String>,
    assistant_text_items: HashMap<KernelStreamItemKey, String>,
    completed_assistant_text_items: HashMap<KernelStreamItemKey, String>,
    active_tool_items: HashMap<String, String>,
    tool_item_by_call_id: HashMap<String, String>,
    completed_tool_call_ids: HashSet<String>,
}

fn execution_from_general_turn_result(
    result: &GeneralTurnResult,
) -> (String, Vec<TalosOutputItem>, TalosStopReason, TalosUsage) {
    match &result.outcome {
        GeneralTurnOutcome::Completed {
            output_text,
            output_items,
            stop_reason,
            usage,
            ..
        } => (
            output_text.clone(),
            output_items.clone(),
            kernel::map_stop_reason(*stop_reason),
            usage.clone(),
        ),
        GeneralTurnOutcome::Waiting {
            output_items,
            stop_reason,
            usage,
            ..
        } => (
            String::new(),
            output_items.clone(),
            kernel::map_stop_reason(*stop_reason),
            usage.clone(),
        ),
    }
}

fn outcome_stop_reason(outcome: &GeneralTurnOutcome) -> KernelStopReason {
    match outcome {
        GeneralTurnOutcome::Completed { stop_reason, .. }
        | GeneralTurnOutcome::Waiting { stop_reason, .. } => *stop_reason,
    }
}

fn outcome_usage(outcome: &GeneralTurnOutcome) -> TalosUsage {
    match outcome {
        GeneralTurnOutcome::Completed { usage, .. } | GeneralTurnOutcome::Waiting { usage, .. } => {
            usage.clone()
        }
    }
}

fn outcome_metrics(outcome: &GeneralTurnOutcome) -> TurnExecutionMetrics {
    match outcome {
        GeneralTurnOutcome::Completed { metrics, .. }
        | GeneralTurnOutcome::Waiting { metrics, .. } => metrics.clone(),
    }
}

fn seed_resume_context(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    resume_context: Option<GatewayResumeContext>,
) {
    let Some(tool_call) = resume_context.and_then(|context| context.tool_call) else {
        return;
    };
    timeline.track_existing_tool_call(
        tool_call.item_id.clone(),
        tool_call.call_id.clone(),
        tool_call.tool_name,
        String::new(),
    );
    state
        .tool_item_by_call_id
        .insert(tool_call.call_id.clone(), tool_call.item_id.clone());
    state
        .active_tool_items
        .insert(tool_call.call_id, tool_call.item_id);
}

fn emit_kernel_timeline_events(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    event: KernelStreamEvent,
) -> TalosResult<Vec<TalosTimelineEvent>> {
    match (event.event_type, event.data) {
        (KernelStreamEventType::TurnMetrics, KernelStreamEventData::TurnMetrics(data)) => {
            Ok(vec![timeline.turn_metrics(data.metrics)])
        }
        (KernelStreamEventType::ThinkingDelta, KernelStreamEventData::ThinkingDelta(data)) => {
            reject_after_final_answer(state, "thinking delta")?;
            append_text_item_delta(timeline, &mut state.reasoning_item_id, data.delta)
        }
        (
            KernelStreamEventType::AssistantTextStarted,
            KernelStreamEventData::AssistantTextStarted(data),
        ) => {
            if state.assistant_text_items.contains_key(&data.item_key) {
                return Err(TalosError::ValidationError(format!(
                    "timeline assistant text item already started: {:?}",
                    data.item_key
                )));
            }
            state.completed_assistant_text_items.remove(&data.item_key);
            if matches!(data.phase, KernelAssistantTextPhase::FinalAnswer)
                && state.final_answer_item_id.is_some()
            {
                return Err(TalosError::ValidationError(
                    "timeline final answer item already started".to_string(),
                ));
            }
            if matches!(data.phase, KernelAssistantTextPhase::Commentary) {
                reject_after_final_answer(state, "commentary assistant text")?;
            }
            let started = match data.phase {
                KernelAssistantTextPhase::Unknown => {
                    reject_after_final_answer(state, "unknown assistant text")?;
                    timeline.start_unknown_assistant_text()
                }
                KernelAssistantTextPhase::Commentary => timeline.start_commentary(),
                KernelAssistantTextPhase::FinalAnswer => timeline.start_final_answer(),
            };
            let item_id = item_id_from_started(&started)?;
            if matches!(data.phase, KernelAssistantTextPhase::FinalAnswer) {
                state.final_answer_item_id = Some(item_id.clone());
            }
            state.assistant_text_items.insert(data.item_key, item_id);
            Ok(vec![started])
        }
        (
            KernelStreamEventType::AssistantTextDelta,
            KernelStreamEventData::AssistantTextDelta(data),
        ) => {
            let item_id = state
                .assistant_text_items
                .get(&data.item_key)
                .cloned()
                .ok_or_else(|| {
                    TalosError::ValidationError(format!(
                        "timeline assistant text item not found for delta: {:?}",
                        data.item_key
                    ))
                })?;
            Ok(vec![timeline.append_delta(
                &item_id,
                TalosTimelineDelta::Text { text: data.delta },
            )?])
        }
        (
            KernelStreamEventType::AssistantTextCompleted,
            KernelStreamEventData::AssistantTextCompleted(data),
        ) => {
            let item_id = state
                .assistant_text_items
                .remove(&data.item_key)
                .ok_or_else(|| {
                    TalosError::ValidationError(format!(
                        "timeline assistant text item not found for completion: {:?}",
                        data.item_key
                    ))
                })?;
            Ok(vec![timeline.complete_item(
                &item_id,
                TalosTimelineItemStatus::Completed,
                None,
            )?])
            .map(|events| {
                state
                    .completed_assistant_text_items
                    .insert(data.item_key, item_id);
                events
            })
        }
        (
            KernelStreamEventType::AssistantTextCommitted,
            KernelStreamEventData::AssistantTextCommitted(data),
        ) => {
            if matches!(data.phase, KernelAssistantTextPhase::Unknown) {
                return Err(TalosError::ValidationError(
                    "timeline assistant text commit phase cannot be unknown".to_string(),
                ));
            }
            let item_id = state
                .assistant_text_items
                .get(&data.item_key)
                .or_else(|| state.completed_assistant_text_items.get(&data.item_key))
                .cloned()
                .ok_or_else(|| {
                    TalosError::ValidationError(format!(
                        "timeline assistant text item not found for commit: {:?}",
                        data.item_key
                    ))
                })?;
            let phase = match data.phase {
                KernelAssistantTextPhase::Unknown => unreachable!(),
                KernelAssistantTextPhase::Commentary => {
                    reject_after_final_answer(state, "commentary assistant text commit")?;
                    TalosTimelinePhase::Commentary
                }
                KernelAssistantTextPhase::FinalAnswer => {
                    if state
                        .final_answer_item_id
                        .as_ref()
                        .is_some_and(|existing| existing != &item_id)
                    {
                        return Err(TalosError::ValidationError(
                            "timeline final answer item already started".to_string(),
                        ));
                    }
                    state.final_answer_item_id = Some(item_id.clone());
                    TalosTimelinePhase::FinalAnswer
                }
            };
            Ok(vec![timeline.commit_assistant_text(&item_id, phase)?])
        }
        (
            KernelStreamEventType::ExecutionSummary,
            KernelStreamEventData::ExecutionSummary(data),
        ) => {
            reject_after_final_answer(state, "execution summary")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let started = timeline.start_item(
                TalosTimelineItemKind::ExecutionSummary,
                TalosTimelinePhase::System,
                Some("Execution summary".to_string()),
                TalosTimelineContent::ExecutionSummary { summary: data.text },
            );
            let item_id = item_id_from_started(&started)?;
            let completed =
                timeline.complete_item(&item_id, TalosTimelineItemStatus::Completed, None)?;
            events.push(started);
            events.push(completed);
            Ok(events)
        }
        (KernelStreamEventType::ToolCallStart, KernelStreamEventData::ToolCallStart(data)) => {
            reject_after_final_answer(state, "tool call start")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let stream_key = ToolStreamKey {
                round: data.round,
                index: data.index,
            };
            let call_id = data
                .call_id
                .unwrap_or_else(|| stream_key_fallback(&stream_key));
            let tool_name = data.tool_name.unwrap_or_else(|| "unknown".to_string());
            let (_, started) = ensure_tool_item(
                timeline,
                state,
                stream_key,
                Some(&call_id),
                Some(&tool_name),
                data.arguments.to_string(),
            )?;
            events.extend(started);
            Ok(events)
        }
        (KernelStreamEventType::ToolArgsDelta, KernelStreamEventData::ToolArgsDelta(data)) => {
            reject_after_final_answer(state, "tool arguments delta")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let (item_id, started) = ensure_tool_item(
                timeline,
                state,
                data.stream_key,
                data.call_id.as_deref(),
                data.tool_name.as_deref(),
                String::new(),
            )?;
            let delta = timeline.append_delta(
                &item_id,
                TalosTimelineDelta::ToolArguments {
                    arguments_delta: data.delta,
                },
            )?;
            events.extend(optional_started_with_delta(started, delta));
            Ok(events)
        }
        (KernelStreamEventType::ToolQueued, KernelStreamEventData::ToolQueued(data)) => {
            reject_after_final_answer(state, "tool queued")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let (item_id, started) = ensure_tool_item(
                timeline,
                state,
                data.stream_key,
                Some(&data.call_id),
                Some(&data.tool_name),
                String::new(),
            )?;
            let delta = timeline.append_delta(
                &item_id,
                TalosTimelineDelta::Status {
                    status: TalosTimelineItemStatus::Running,
                },
            )?;
            events.extend(optional_started_with_delta(started, delta));
            Ok(events)
        }
        (KernelStreamEventType::ToolExecuting, KernelStreamEventData::ToolExecuting(data)) => {
            reject_after_final_answer(state, "tool executing")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let (item_id, started) = ensure_tool_item(
                timeline,
                state,
                data.stream_key,
                Some(&data.call_id),
                Some(&data.tool_name),
                String::new(),
            )?;
            let delta = timeline.append_delta(
                &item_id,
                TalosTimelineDelta::ToolProgress {
                    progress_kind: TalosToolProgressKind::Status,
                    text_delta: format!("Executing {}", data.tool_name),
                },
            )?;
            events.extend(optional_started_with_delta(started, delta));
            Ok(events)
        }
        (
            KernelStreamEventType::ToolProgressDelta,
            KernelStreamEventData::ToolProgressDelta(data),
        ) => {
            reject_after_final_answer(state, "tool progress delta")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let (item_id, started) = ensure_tool_item(
                timeline,
                state,
                data.stream_key,
                Some(&data.call_id),
                Some(&data.tool_name),
                String::new(),
            )?;
            let delta = timeline.append_delta(
                &item_id,
                TalosTimelineDelta::ToolProgress {
                    progress_kind: map_timeline_tool_progress_kind(data.progress_kind),
                    text_delta: data.message,
                },
            )?;
            events.extend(optional_started_with_delta(started, delta));
            Ok(events)
        }
        (KernelStreamEventType::ToolResult, KernelStreamEventData::ToolResult(data)) => {
            reject_after_final_answer(state, "tool result")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let (item_id, started) =
                ensure_tool_item_for_call(timeline, state, &data.call_id, &data.tool_name);
            let result = timeline.append_delta(
                &item_id,
                TalosTimelineDelta::ToolResult {
                    content: if data.content.is_empty() {
                        vec![TalosContentItem::Json {
                            value: serde_json::Value::Array(Vec::new()).into(),
                        }]
                    } else {
                        data.content
                    },
                    is_error: data.is_error,
                },
            )?;
            if data.next_turn_failure.is_some() {
                events.extend(optional_started_with_delta(started, result));
                return Ok(events);
            }
            let completed = timeline.complete_item(
                &item_id,
                if data.is_error {
                    TalosTimelineItemStatus::Failed
                } else {
                    TalosTimelineItemStatus::Completed
                },
                None,
            )?;
            state.completed_tool_call_ids.insert(data.call_id);
            events.extend(optional_started_with_delta(started, result));
            events.push(completed);
            Ok(events)
        }
        (
            KernelStreamEventType::DynamicToolRequest,
            KernelStreamEventData::DynamicToolRequest(data),
        ) => {
            reject_after_final_answer(state, "dynamic tool request")?;
            let mut events = close_active_reasoning_item(timeline, state)?;
            let item_id = state
                .tool_item_by_call_id
                .get(&data.call_id)
                .cloned()
                .ok_or_else(|| {
                    TalosError::ValidationError(format!(
                        "timeline tool item not found for dynamic result: {}",
                        data.call_id
                    ))
                })?;
            events.push(timeline.append_delta(
                &item_id,
                TalosTimelineDelta::Status {
                    status: TalosTimelineItemStatus::WaitingDynamicInput,
                },
            )?);
            events.push(timeline.append_delta(
                &item_id,
                TalosTimelineDelta::ToolDynamicRequest {
                    call_id: data.call_id,
                    tool_name: data.tool_name,
                    arguments: data.arguments.into(),
                },
            )?);
            Ok(events)
        }
        (KernelStreamEventType::PromptWarning, KernelStreamEventData::PromptWarning(_)) => {
            Ok(Vec::new())
        }
        _ => Ok(Vec::new()),
    }
}

fn reject_after_final_answer(state: &KernelTimelineState, event_name: &str) -> TalosResult<()> {
    if state.final_answer_item_id.is_some() {
        return Err(TalosError::ValidationError(format!(
            "timeline event after final answer is not allowed: {event_name}"
        )));
    }
    Ok(())
}

fn close_active_reasoning_item(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
) -> TalosResult<Vec<TalosTimelineEvent>> {
    let Some(item_id) = state.reasoning_item_id.clone() else {
        return Ok(Vec::new());
    };
    let completed = timeline.complete_item(&item_id, TalosTimelineItemStatus::Completed, None)?;
    state.reasoning_item_id = None;
    Ok(vec![completed])
}

fn append_text_item_delta(
    timeline: &mut TimelineBuilder,
    item_id_slot: &mut Option<String>,
    text: String,
) -> TalosResult<Vec<TalosTimelineEvent>> {
    let mut events = Vec::new();
    let item_id = match item_id_slot.clone() {
        Some(item_id) => item_id,
        None => {
            let started = timeline.start_reasoning();
            let item_id = item_id_from_started(&started)?;
            *item_id_slot = Some(item_id.clone());
            events.push(started);
            item_id
        }
    };
    events.push(timeline.append_delta(&item_id, TalosTimelineDelta::Text { text })?);
    Ok(events)
}

fn emit_missing_tool_items_from_result(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    output_items: &[TalosOutputItem],
) -> TalosResult<Vec<TalosTimelineEvent>> {
    let mut events = Vec::new();
    for output_item in output_items {
        match output_item {
            TalosOutputItem::ToolCall {
                call_id,
                name,
                arguments,
            } if !state.tool_item_by_call_id.contains_key(call_id) => {
                events.extend(close_active_reasoning_item(timeline, state)?);
                let started = timeline.start_tool_call(call_id, name, arguments.to_string());
                let item_id = item_id_from_started(&started)?;
                state
                    .tool_item_by_call_id
                    .insert(call_id.clone(), item_id.clone());
                state.active_tool_items.insert(call_id.clone(), item_id);
                events.push(started);
            }
            TalosOutputItem::ToolResult {
                call_id,
                tool_name,
                content,
                is_error,
            } if !state.completed_tool_call_ids.contains(call_id) => {
                events.extend(close_active_reasoning_item(timeline, state)?);
                let item_id = match state.tool_item_by_call_id.get(call_id).cloned() {
                    Some(item_id) => item_id,
                    None => {
                        let started = timeline.start_tool_call(call_id, tool_name, String::new());
                        let item_id = item_id_from_started(&started)?;
                        state
                            .tool_item_by_call_id
                            .insert(call_id.clone(), item_id.clone());
                        state
                            .active_tool_items
                            .insert(call_id.clone(), item_id.clone());
                        events.push(started);
                        item_id
                    }
                };
                let result = timeline.append_delta(
                    &item_id,
                    TalosTimelineDelta::ToolResult {
                        content: content.clone(),
                        is_error: *is_error,
                    },
                )?;
                events.push(result);
                events.push(timeline.complete_item(
                    &item_id,
                    if *is_error {
                        TalosTimelineItemStatus::Failed
                    } else {
                        TalosTimelineItemStatus::Completed
                    },
                    None,
                )?);
                state.completed_tool_call_ids.insert(call_id.clone());
            }
            _ => {}
        }
    }
    Ok(events)
}

fn item_id_from_started(event: &TalosTimelineEvent) -> TalosResult<String> {
    match &event.payload {
        TalosTimelineEventPayload::ItemStarted(payload) => Ok(payload.item.item_id.clone()),
        _ => Err(TalosError::InternalError(
            "expected item started timeline event".to_string(),
        )),
    }
}

fn tool_item_id(
    state: &KernelTimelineState,
    stream_key: ToolStreamKey,
    call_id: Option<&str>,
) -> TalosResult<String> {
    if let Some(call_id) = call_id {
        if let Some(item_id) = state.tool_item_by_call_id.get(call_id) {
            return Ok(item_id.clone());
        }
    }
    state
        .active_tool_items
        .get(&stream_key_key(&stream_key))
        .cloned()
        .ok_or_else(|| {
            TalosError::ValidationError(format!(
                "timeline tool item not found for stream key round={} index={}",
                stream_key.round, stream_key.index
            ))
        })
}

fn ensure_tool_item(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    stream_key: ToolStreamKey,
    call_id: Option<&str>,
    tool_name: Option<&str>,
    arguments: String,
) -> TalosResult<(String, Option<TalosTimelineEvent>)> {
    if let Ok(item_id) = tool_item_id(state, stream_key.clone(), call_id) {
        return Ok((item_id, None));
    }

    let call_id = call_id
        .map(ToString::to_string)
        .unwrap_or_else(|| stream_key_fallback(&stream_key));
    let tool_name = tool_name.unwrap_or("unknown");
    let started = timeline.start_tool_call(call_id.clone(), tool_name, arguments);
    let item_id = item_id_from_started(&started)?;
    state
        .active_tool_items
        .insert(stream_key_key(&stream_key), item_id.clone());
    state.tool_item_by_call_id.insert(call_id, item_id.clone());
    Ok((item_id, Some(started)))
}

fn ensure_tool_item_for_call(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    call_id: &str,
    tool_name: &str,
) -> (String, Option<TalosTimelineEvent>) {
    if let Some(item_id) = state.tool_item_by_call_id.get(call_id).cloned() {
        return (item_id, None);
    }

    let started = timeline.start_tool_call(call_id, tool_name, String::new());
    let item_id = match item_id_from_started(&started) {
        Ok(item_id) => item_id,
        Err(_) => return (call_id.to_string(), Some(started)),
    };
    state
        .tool_item_by_call_id
        .insert(call_id.to_string(), item_id.clone());
    state
        .active_tool_items
        .insert(call_id.to_string(), item_id.clone());
    (item_id, Some(started))
}

fn optional_started_with_delta(
    started: Option<TalosTimelineEvent>,
    delta: TalosTimelineEvent,
) -> Vec<TalosTimelineEvent> {
    match started {
        Some(started) => vec![started, delta],
        None => vec![delta],
    }
}

fn map_timeline_tool_progress_kind(kind: ToolProgressKind) -> TalosToolProgressKind {
    match kind {
        ToolProgressKind::Status => TalosToolProgressKind::Status,
        ToolProgressKind::Stdout => TalosToolProgressKind::Stdout,
        ToolProgressKind::Stderr => TalosToolProgressKind::Stderr,
    }
}

fn stream_key_fallback(stream_key: &ToolStreamKey) -> String {
    format!("tool_{}_{}", stream_key.round, stream_key.index)
}

fn stream_key_key(stream_key: &ToolStreamKey) -> String {
    format!("{}:{}", stream_key.round, stream_key.index)
}

#[async_trait]
impl AgentOrchestrator for AgentHarness {
    fn id(&self) -> &str {
        &self.id
    }

    fn take_session_title_update(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Option<SessionTitleUpdate> {
        self.completed_title_updates
            .remove(&Self::title_update_key(session_id, turn_id))
            .map(|(_, update)| update)
    }

    async fn run(&self, request: OrchestratorRequest) -> TalosResult<OrchestratorExecution> {
        if let Some(response_rx) = self.reserve_or_enqueue_unary_turn(request.clone()).await? {
            return response_rx.await.map_err(|err| {
                TalosError::InternalError(format!(
                    "queued orchestrator execution channel closed: {}",
                    err
                ))
            })?;
        }

        self.execute_reserved_run(request).await
    }

    async fn run_stream(
        &self,
        request: OrchestratorRequest,
    ) -> TalosResult<mpsc::Receiver<TalosTimelineEvent>> {
        let (tx, rx) = mpsc::channel(256);
        let should_start_now = self
            .reserve_or_enqueue_stream_turn(request.clone(), tx.clone())
            .await?;
        if should_start_now {
            self.spawn_reserved_stream_turn(request, tx);
        }
        Ok(rx)
    }

    async fn close_session(&self, session: talos_types::TalosSession) -> TalosResult<()> {
        self.close_session_impl(session).await
    }

    async fn compact_session(&self, session: talos_types::TalosSession) -> TalosResult<TalosUsage> {
        self.compact_session_impl(session).await
    }

    async fn clear_session(&self, session: talos_types::TalosSession) -> TalosResult<()> {
        self.clear_session_impl(session).await
    }

    async fn drain_memory_flushes(&self, timeout: Duration) -> TalosResult<()> {
        self.drain_pending_memory_flushes(timeout).await
    }

    fn pending_memory_flushes(&self) -> usize {
        self.pending_memory_flush_count()
    }

    async fn has_pending_memory_flush(&self, session_id: &str) -> bool {
        AgentHarness::has_pending_memory_flush(self, session_id).await
    }

    fn has_active_turn(&self, session_id: &str) -> bool {
        AgentHarness::has_active_turn(self, session_id)
    }

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        let Some(handle) = self.active_turns.get(session_id) else {
            return Ok(());
        };
        if let ActiveTurnHandle::Running { execution_task } = handle.value() {
            execution_task.abort();
        }
        Ok(())
    }

    async fn drain_active_turns(&self, timeout: Duration) -> TalosResult<()> {
        AgentHarness::drain_active_turns(self, timeout).await
    }

    async fn submit_dynamic_tool_result(
        &self,
        session: talos_types::TalosSession,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        self.submit_dynamic_tool_result(session, call_id, tool_name, output, success)
            .await
    }
}
