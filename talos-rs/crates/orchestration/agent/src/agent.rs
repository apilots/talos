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
mod tooling;

pub use mock::MockAgent;

use crate::{
    flush_manager::MemoryFlushManager, Agent, AgentOrchestrator, AgentResponse,
    OrchestratorExecution, OrchestratorRequest,
};
use talos_agent_kernel::{
    ConfirmationRequestStreamData, KernelRuntime, KernelStreamEvent, KernelStreamEventData,
    KernelStreamEventType, RunRequest, TimelineBuilder, ToolProgressKind, ToolStreamKey,
};
use talos_config::{AgentKernelConfig, PlannerMode};
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::LlmProvider;
use talos_memory::MemoryStore;
use talos_tools::ToolRegistry;
use talos_types::gateway::GatewayResumeContext;
use talos_types::talos::{
    StopReason as TalosStopReason, TalosContentItem, TalosOutputItem, TalosTimelineContent,
    TalosTimelineDelta, TalosTimelineEvent, TalosTimelineEventPayload, TalosTimelineItemKind,
    TalosTimelineItemStatus, TalosTimelinePhase, TalosToolProgressKind, TalosUsage,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KernelExecutionMode {
    Single,
    Sequential,
    Parallel,
    Loop,
}

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
pub struct AgentImpl {
    pub id: String,
    pub name: String,
    pub description: String,
    pub llm_provider: Arc<dyn LlmProvider>,
    pub model: String,
    pub memory_store: Arc<MemoryStore>,
    pub tool_registry: Arc<ToolRegistry>,
    kernel_config: AgentKernelConfig,
    pub kernel_runtime: Arc<KernelRuntime>,
    active_turns: Arc<DashMap<String, ActiveTurnHandle>>,
    session_backlogs: Arc<Mutex<HashMap<String, VecDeque<QueuedTurn>>>>,
    memory_flush_manager: Arc<MemoryFlushManager>,
}

impl AgentImpl {
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
            kernel_runtime: Arc::new(KernelRuntime::new_default()),
            active_turns: Arc::new(DashMap::new()),
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
        let agent_request = self.to_agent_request(request);
        let input = Self::extract_text_input(&agent_request.input);
        let invocation_id = format!("inv-{}", Utc::now().timestamp_millis());
        let session_id = agent_request.session.context.session_id.clone();
        let message_id = agent_request.session.context.message_id.clone();
        let selected_model_info = self.resolve_selected_model_info(&agent_request.session);
        let phase = agent_request.phase.clone();
        let planning_instruction = agent_request.planning_instruction.clone();
        let transfer_to = agent_request.transfer_to.clone();
        let kernel_config = agent_request.session.config.agent.kernel.clone();
        let kernel_agent = match self.build_kernel_agent_for_request(&kernel_config) {
            Ok(agent) => agent,
            Err(err) => {
                self.finish_active_turn_and_resume(&session_id).await;
                return Err(err);
            }
        };
        let runtime = self.kernel_runtime.clone();
        let agent_id = self.id.clone();
        let run_session_id = session_id.clone();
        let run_handle = tokio::spawn({
            async move {
                runtime
                    .run_turn(
                        kernel_agent,
                        RunRequest {
                            session_id: run_session_id,
                            message_id,
                            invocation_id,
                            agent_id,
                            workspace_id: agent_request
                                .session
                                .context
                                .workspace_id
                                .clone()
                                .unwrap_or_default(),
                            project_root: agent_request.session.context.project_root.clone(),
                            input,
                            selected_model_info,
                            phase,
                            planning_instruction,
                            transfer_to,
                            initial_state_bags: None,
                            session_environment: agent_request.session.context.environment.clone(),
                            kernel_config: Some(kernel_config),
                            stream_tx: None,
                        },
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
                let reasoning = Self::collect_reasoning(&result.output_items);
                Ok(OrchestratorExecution {
                    output_text: result.output_text,
                    output_items: result.output_items,
                    reasoning,
                    stop_reason: kernel::map_stop_reason(result.stop_reason),
                    usage: result.usage,
                    task_count: result.task_results.len() as u32,
                    session_id: session_id.clone(),
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
            let mut timeline = TimelineBuilder::new(
                request_id.clone(),
                turn_id,
                message_id.clone(),
                session_id.clone(),
            );
            let mut timeline_state = KernelTimelineState::default();
            seed_resume_context(&mut timeline, &mut timeline_state, resume_context);
            let selected_model_info = this.resolve_selected_model_info(&agent_request.session);
            let phase = agent_request.phase.clone();
            let planning_instruction = agent_request.planning_instruction.clone();
            let transfer_to = agent_request.transfer_to.clone();
            let kernel_config = agent_request.session.config.agent.kernel.clone();
            let kernel_agent = match this.build_kernel_agent_for_request(&kernel_config) {
                Ok(agent) => agent,
                Err(err) => {
                    let _ = tx.send(timeline.fail_turn(err.to_string())).await;
                    this.finish_active_turn_and_resume(&session_id).await;
                    return;
                }
            };
            let (kernel_tx, mut kernel_rx) = mpsc::channel(256);
            let runtime = this.kernel_runtime.clone();
            let kernel_input = RunRequest {
                session_id: agent_request.session.context.session_id.clone(),
                message_id,
                invocation_id,
                agent_id: this.id.clone(),
                workspace_id: agent_request
                    .session
                    .context
                    .workspace_id
                    .clone()
                    .unwrap_or_default(),
                project_root: agent_request.session.context.project_root.clone(),
                input: input.clone(),
                selected_model_info,
                phase,
                planning_instruction,
                transfer_to,
                initial_state_bags: None,
                session_environment: agent_request.session.context.environment.clone(),
                kernel_config: Some(kernel_config),
                stream_tx: Some(kernel_tx),
            };

            let _ = tx.send(timeline.start_turn()).await;
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

            let run_handle =
                tokio::spawn(async move { runtime.run_turn(kernel_agent, kernel_input).await });
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
                    if !result.output_text.trim().is_empty()
                        && timeline_state.final_answer_item_id.is_none()
                    {
                        match emit_final_answer_from_result(
                            &mut timeline,
                            &mut timeline_state,
                            result.output_text,
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
                    }
                    emit_terminal_item_closures(
                        &mut timeline,
                        &tx,
                        TalosTimelineItemStatus::Completed,
                    )
                    .await;
                    let _ = tx
                        .send(timeline.complete_turn(
                            kernel::map_stop_reason(result.stop_reason),
                            result.usage,
                        ))
                        .await;
                }
                Err(err) if err.is_cancelled() => {
                    emit_terminal_item_closures(
                        &mut timeline,
                        &tx,
                        TalosTimelineItemStatus::Cancelled,
                    )
                    .await;
                    let _ = tx
                        .send(
                            timeline
                                .complete_turn(TalosStopReason::Cancelled, TalosUsage::default()),
                        )
                        .await;
                }
                Ok(Err(err)) => {
                    emit_terminal_item_closures(
                        &mut timeline,
                        &tx,
                        TalosTimelineItemStatus::Failed,
                    )
                    .await;
                    let _ = tx.send(timeline.fail_turn(err.to_string())).await;
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
                }
            }
            this.finish_active_turn_and_resume(&session_id).await;
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
    active_tool_items: HashMap<String, String>,
    tool_item_by_call_id: HashMap<String, String>,
    completed_tool_call_ids: HashSet<String>,
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
        tool_call.arguments.to_string(),
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
        (KernelStreamEventType::ThinkingDelta, KernelStreamEventData::ThinkingDelta(data)) => {
            append_text_item_delta(
                timeline,
                &mut state.reasoning_item_id,
                TimelineTextItem::Reasoning,
                data.delta,
            )
        }
        (KernelStreamEventType::OutputDelta, KernelStreamEventData::OutputDelta(data)) => {
            append_text_item_delta(
                timeline,
                &mut state.final_answer_item_id,
                TimelineTextItem::FinalAnswer,
                data.delta,
            )
        }
        (
            KernelStreamEventType::ExecutionSummary,
            KernelStreamEventData::ExecutionSummary(data),
        ) => Ok(vec![timeline.start_item(
            TalosTimelineItemKind::ExecutionSummary,
            TalosTimelinePhase::System,
            Some("Execution summary".to_string()),
            TalosTimelineContent::ExecutionSummary { summary: data.text },
        )]),
        (KernelStreamEventType::ToolCallStart, KernelStreamEventData::ToolCallStart(data)) => {
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
            Ok(started.into_iter().collect())
        }
        (KernelStreamEventType::ToolArgsDelta, KernelStreamEventData::ToolArgsDelta(data)) => {
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
            Ok(optional_started_with_delta(started, delta))
        }
        (KernelStreamEventType::ToolQueued, KernelStreamEventData::ToolQueued(data)) => {
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
            Ok(optional_started_with_delta(started, delta))
        }
        (KernelStreamEventType::ToolExecuting, KernelStreamEventData::ToolExecuting(data)) => {
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
            Ok(optional_started_with_delta(started, delta))
        }
        (
            KernelStreamEventType::ToolProgressDelta,
            KernelStreamEventData::ToolProgressDelta(data),
        ) => {
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
            Ok(optional_started_with_delta(started, delta))
        }
        (KernelStreamEventType::ToolResult, KernelStreamEventData::ToolResult(data)) => {
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
                return Ok(optional_started_with_delta(started, result));
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
            let mut events = optional_started_with_delta(started, result);
            events.push(completed);
            Ok(events)
        }
        (
            KernelStreamEventType::ConfirmationRequest,
            KernelStreamEventData::ConfirmationRequest(ConfirmationRequestStreamData {
                call_id,
                tool_name,
                prompt,
                arguments,
            }),
        ) => {
            let item_id = state
                .tool_item_by_call_id
                .get(&call_id)
                .cloned()
                .ok_or_else(|| {
                    TalosError::ValidationError(format!(
                        "timeline tool item not found for approval: {call_id}"
                    ))
                })?;
            let delta = TalosTimelineDelta::ToolApprovalRequest {
                call_id: call_id.clone(),
                tool_name: tool_name.clone(),
                prompt: prompt.clone(),
                arguments: arguments.clone().into(),
            };
            Ok(vec![
                timeline.append_delta(
                    &item_id,
                    TalosTimelineDelta::Status {
                        status: TalosTimelineItemStatus::WaitingApproval,
                    },
                )?,
                timeline.append_delta(&item_id, delta)?,
            ])
        }
        (
            KernelStreamEventType::DynamicToolRequest,
            KernelStreamEventData::DynamicToolRequest(data),
        ) => {
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
            Ok(vec![
                timeline.append_delta(
                    &item_id,
                    TalosTimelineDelta::Status {
                        status: TalosTimelineItemStatus::WaitingApproval,
                    },
                )?,
                timeline.append_delta(
                    &item_id,
                    TalosTimelineDelta::ToolDynamicRequest {
                        call_id: data.call_id,
                        tool_name: data.tool_name,
                        arguments: data.arguments.into(),
                    },
                )?,
            ])
        }
        (KernelStreamEventType::PromptWarning, KernelStreamEventData::PromptWarning(_)) => {
            Ok(Vec::new())
        }
        _ => Ok(Vec::new()),
    }
}

enum TimelineTextItem {
    Reasoning,
    FinalAnswer,
}

fn append_text_item_delta(
    timeline: &mut TimelineBuilder,
    item_id_slot: &mut Option<String>,
    item: TimelineTextItem,
    text: String,
) -> TalosResult<Vec<TalosTimelineEvent>> {
    let mut events = Vec::new();
    let item_id = match item_id_slot.clone() {
        Some(item_id) => item_id,
        None => {
            let started = match item {
                TimelineTextItem::Reasoning => timeline.start_reasoning(),
                TimelineTextItem::FinalAnswer => timeline.start_final_answer(),
            };
            let item_id = item_id_from_started(&started)?;
            *item_id_slot = Some(item_id.clone());
            events.push(started);
            item_id
        }
    };
    events.push(timeline.append_delta(&item_id, TalosTimelineDelta::Text { text })?);
    Ok(events)
}

fn emit_final_answer_from_result(
    timeline: &mut TimelineBuilder,
    state: &mut KernelTimelineState,
    text: String,
) -> TalosResult<Vec<TalosTimelineEvent>> {
    let mut events = append_text_item_delta(
        timeline,
        &mut state.final_answer_item_id,
        TimelineTextItem::FinalAnswer,
        text,
    )?;
    if let Some(item_id) = state.final_answer_item_id.as_deref() {
        events.push(timeline.complete_item(item_id, TalosTimelineItemStatus::Completed, None)?);
    }
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
impl Agent for AgentImpl {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> TalosResult<AgentResponse> {
        self.execute_tool_internal(tool_name, args).await
    }
}

#[async_trait]
impl AgentOrchestrator for AgentImpl {
    fn id(&self) -> &str {
        &self.id
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
        AgentImpl::has_pending_memory_flush(self, session_id).await
    }

    fn has_active_turn(&self, session_id: &str) -> bool {
        AgentImpl::has_active_turn(self, session_id)
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
        AgentImpl::drain_active_turns(self, timeout).await
    }

    async fn submit_tool_approval(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        approved: bool,
        reason: Option<String>,
    ) -> TalosResult<()> {
        self.submit_tool_approval(session_id, call_id, tool_name, approved, reason)
            .await
    }

    async fn submit_dynamic_tool_result(
        &self,
        session_id: &str,
        call_id: &str,
        tool_name: &str,
        output: String,
        success: bool,
    ) -> TalosResult<()> {
        self.submit_dynamic_tool_result(session_id, call_id, tool_name, output, success)
            .await
    }
}
