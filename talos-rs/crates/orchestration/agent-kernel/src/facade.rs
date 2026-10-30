use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_llm_provider::LlmProviderRouter;
use talos_memory::DreamMemoryFormationStore;
use talos_prompt::UserWorkspaceMemoryPromptContext;
use talos_run_control::{RunTask, RunTaskCompletion, RunTaskGroup, SpawnError};
use talos_tools::{ToolDefinitionProvider, ToolExecutorPort};
use talos_types::talos::TalosSessionEnvironment;
use talos_types::{
    ContextUsage, ModelInfo, ModelSelection, TurnExecutionMetrics, UserActionParams,
};
use tokio::sync::mpsc;

use crate::{
    run_general_turn_with_resume, ActiveTurnInputDelivery, AgentKernelContinuation,
    AgentKernelEvent, AgentKernelEventPayload, GeneralTurnOutcome, GeneralTurnResult,
    GeneralTurnSkillOptions, InvocationContext, KernelMemoryProgressStore, KernelRunItem,
    KernelSessionArtifactStore, KernelTranscriptService, ResolvedAttachment,
    SessionPromptContextSource, StopReason, ToolCallStartStreamData, ToolResultStreamData,
    TurnPhase, UserInput,
};

const KERNEL_EVENT_CAPACITY: usize = 256;

/// Dependencies used by one configured Agent Kernel instance.
///
/// Concrete Provider adapters resolve model, Tool, memory, and prompt ports
/// before constructing this facade. Product Service, Store, Harness, and
/// transport objects must not be added here.
#[derive(Clone)]
pub struct AgentKernelDependencies {
    pub agent_name: String,
    pub agent_description: String,
    pub llm_provider_router: Arc<dyn LlmProviderRouter>,
    pub tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    pub tool_executor: Arc<dyn ToolExecutorPort>,
    pub dream_memory_store: Arc<dyn DreamMemoryFormationStore>,
    pub transcript: Arc<dyn KernelTranscriptService>,
    pub artifacts: Arc<dyn KernelSessionArtifactStore>,
    pub prompt_context: Arc<dyn SessionPromptContextSource>,
    pub memory_progress: Arc<dyn KernelMemoryProgressStore>,
}

/// Dependency-owning facade for the Kernel reasoning loop.
///
/// This is not an Agent Harness or Provider: it has no admission, routing,
/// terminal Agent event, product Session lifecycle, or persistence API.
#[derive(Clone)]
pub struct AgentKernel {
    trajectory: Option<Arc<dyn crate::trajectory::KernelTrajectoryRecorder>>,
    dependencies: AgentKernelDependencies,
}

impl AgentKernel {
    /// Create one Kernel facade from already validated execution dependencies.
    pub fn new(dependencies: AgentKernelDependencies) -> Self {
        Self {
            dependencies,
            trajectory: None,
        }
    }

    /// Install invocation-local observation without changing canonical events.
    #[must_use]
    pub fn with_trajectory_recorder(
        mut self,
        recorder: Arc<dyn crate::trajectory::KernelTrajectoryRecorder>,
    ) -> Self {
        self.trajectory = Some(recorder);
        self
    }

    /// Start one Kernel invocation and return its event/result channels.
    pub fn execute(
        &self,
        request: AgentKernelRequest,
        io: AgentKernelIo,
    ) -> TalosResult<AgentKernelExecution> {
        let (event_tx, event_rx) = mpsc::channel(KERNEL_EVENT_CAPACITY);
        let (kernel_event_tx, mut kernel_event_rx) = mpsc::channel(KERNEL_EVENT_CAPACITY);

        let trajectory = self.trajectory.clone();
        let dependencies = self.dependencies.clone();
        let llm_provider = dependencies
            .llm_provider_router
            .provider_for(&request.model_selection.model_ref)?;
        let run_tasks = io.tasks.clone();
        let result = io
            .tasks
            .spawn(move |cancellation| async move {
                let (
                    mut context,
                    kernel_config,
                    continuation,
                    skill_options,
                    user_workspace_memory,
                ) = match request.into_execution(&dependencies, kernel_event_tx).await {
                    Ok(execution) => execution,
                    Err(error) => return RunTaskCompletion::Confirmed(Err(error)),
                };
                context.trajectory = trajectory;
                let input = context.input.clone();
                let mut emitted_tool_calls = HashSet::new();
                let mut emitted_tool_results = HashSet::new();
                let result = {
                    let run = run_general_turn_with_resume(
                        &mut context,
                        &dependencies.agent_name,
                        &dependencies.agent_description,
                        &input.text,
                        llm_provider,
                        dependencies.tool_definition_provider,
                        dependencies.tool_executor,
                        dependencies.dream_memory_store,
                        user_workspace_memory,
                        &kernel_config,
                        continuation,
                        Some(skill_options),
                        Some(io.input),
                        run_tasks,
                    );
                    tokio::pin!(run);

                    loop {
                        tokio::select! {
                            event = kernel_event_rx.recv() => {
                                let Some(event) = event else {
                                    break run.as_mut().await;
                                };
                                track_tool_event(
                                    &event,
                                    &mut emitted_tool_calls,
                                    &mut emitted_tool_results,
                                );
                                if event_tx.send(event).await.is_err() {
                                    return RunTaskCompletion::Confirmed(Err(TalosError::internal(
                                        "Agent Kernel event receiver closed before execution settled",
                                    )));
                                }
                            }
                            result = &mut run => break result,
                            () = cancellation.requested() => {
                                break Err(TalosError::new(
                                    ErrorCode::Cancelled,
                                    "Agent Kernel cancellation requested",
                                ));
                            }
                        }
                    }
                };
                context.stream_tx.take();
                while let Some(event) = kernel_event_rx.recv().await {
                    track_tool_event(&event, &mut emitted_tool_calls, &mut emitted_tool_results);
                    if event_tx.send(event).await.is_err() {
                        return RunTaskCompletion::Confirmed(Err(TalosError::internal(
                            "Agent Kernel event receiver closed before execution settled",
                        )));
                    }
                }

                let result = match result {
                    Ok(result) => {
                        for event in
                            missing_tool_events(&result, &emitted_tool_calls, &emitted_tool_results)
                        {
                            if event_tx.send(event).await.is_err() {
                                return RunTaskCompletion::Confirmed(Err(TalosError::internal(
                                    "Agent Kernel event receiver closed before execution settled",
                                )));
                            }
                        }
                        if event_tx.send(final_metrics_event(&result)).await.is_err() {
                            return RunTaskCompletion::Confirmed(Err(TalosError::internal(
                                "Agent Kernel event receiver closed before final metrics",
                            )));
                        }
                        Ok(map_result(result))
                    }
                    Err(error) => Err(error),
                };
                RunTaskCompletion::Confirmed(result)
            })
            .map_err(map_spawn_error)?;

        Ok(AgentKernelExecution {
            events: event_rx,
            result,
        })
    }
}

/// Prepared data for one Kernel invocation.
pub struct AgentKernelRequest {
    pub session_id: String,
    pub user_id: String,
    pub turn_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub working_directory: Option<PathBuf>,
    pub input: UserInput,
    pub resolved_attachments: Vec<ResolvedAttachment>,
    pub model_selection: ModelSelection,
    pub model: ModelInfo,
    pub environment: Option<TalosSessionEnvironment>,
    pub kernel_config: talos_config::AgentKernelConfig,
    pub continuation: Option<AgentKernelContinuation>,
    pub skill_options: GeneralTurnSkillOptions,
    pub user_workspace_memory: UserWorkspaceMemoryPromptContext,
}

impl AgentKernelRequest {
    async fn into_execution(
        self,
        dependencies: &AgentKernelDependencies,
        stream_tx: mpsc::Sender<AgentKernelEvent>,
    ) -> TalosResult<(
        InvocationContext,
        talos_config::AgentKernelConfig,
        Option<AgentKernelContinuation>,
        GeneralTurnSkillOptions,
        UserWorkspaceMemoryPromptContext,
    )> {
        let Self {
            session_id,
            user_id,
            turn_id,
            agent_id,
            workspace_id,
            working_directory,
            input,
            resolved_attachments,
            model_selection,
            model,
            environment,
            kernel_config,
            continuation,
            skill_options,
            user_workspace_memory,
        } = self;
        let session = dependencies
            .transcript
            .load_session(&session_id, None, None)
            .await?;
        let context = InvocationContext {
            trajectory: None,
            session_id: session_id.clone(),
            user_id,
            invocation_id: turn_id,
            agent_id: agent_id.clone(),
            workspace_id,
            project_root: working_directory,
            input,
            resolved_attachments,
            model_selection,
            selected_model_info: model,
            phase: TurnPhase::Execution,
            planning_instruction: None,
            session,
            session_environment: environment,
            transcript_service: dependencies.transcript.clone(),
            session_artifact_store: dependencies.artifacts.clone(),
            session_prompt_context_source: dependencies.prompt_context.clone(),
            memory_progress_store: dependencies.memory_progress.clone(),
            stream_tx: Some(stream_tx),
        };
        Ok((
            context,
            kernel_config,
            continuation,
            skill_options,
            user_workspace_memory,
        ))
    }
}

/// Harness-owned live capabilities consumed by one Kernel invocation.
pub struct AgentKernelIo {
    pub input: mpsc::Receiver<ActiveTurnInputDelivery>,
    pub tasks: RunTaskGroup,
}

/// Independently drained Kernel event stream and final result.
pub struct AgentKernelExecution {
    pub events: mpsc::Receiver<AgentKernelEvent>,
    pub result: RunTask<TalosResult<AgentKernelResult>>,
}

/// Provider-neutral completion metadata emitted after the Kernel event tail.
#[derive(Debug, Clone)]
pub enum AgentKernelResult {
    Completed {
        stop_reason: StopReason,
    },
    WaitingForInput {
        pending: Box<AgentKernelPendingInput>,
        metrics: TurnExecutionMetrics,
    },
}

/// Typed input requested by Kernel before the logical Turn can continue.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentKernelPendingInput {
    UserAction {
        action_id: String,
        call_id: String,
        tool_name: String,
        params: UserActionParams,
    },
}

fn map_spawn_error(error: SpawnError) -> TalosError {
    match error {
        SpawnError::Closed => TalosError::new(
            ErrorCode::Cancelled,
            "Agent Kernel operation stopped accepting tasks",
        ),
        SpawnError::CapacityExceeded => TalosError::internal("Agent Kernel task capacity exceeded"),
    }
}

fn map_result(result: GeneralTurnResult) -> AgentKernelResult {
    match result.outcome {
        GeneralTurnOutcome::Completed { stop_reason, .. } => {
            AgentKernelResult::Completed { stop_reason }
        }
        GeneralTurnOutcome::Waiting {
            pending_action,
            metrics,
            ..
        } => AgentKernelResult::WaitingForInput {
            pending: Box::new(AgentKernelPendingInput::UserAction {
                action_id: pending_action.action_id,
                call_id: pending_action.call_id,
                tool_name: pending_action.tool_name,
                params: pending_action.params,
            }),
            metrics,
        },
    }
}

fn measured_context(result: &GeneralTurnResult) -> Option<ContextUsage> {
    let window = &result.prompt_diagnostics.window_usage;
    if result.last_model_usage.total_tokens == 0 || !window.window_resolved {
        return None;
    }
    ContextUsage::new(
        result.last_model_usage.total_tokens,
        u32::try_from(window.context_window).unwrap_or(u32::MAX),
        u32::try_from(window.effective_prompt_budget).unwrap_or(u32::MAX),
        chrono::Utc::now().timestamp(),
    )
}

fn final_metrics_event(result: &GeneralTurnResult) -> AgentKernelEvent {
    let (usage, metrics) = match &result.outcome {
        GeneralTurnOutcome::Completed { usage, metrics, .. }
        | GeneralTurnOutcome::Waiting { usage, metrics, .. } => (usage.clone(), metrics.clone()),
    };
    AgentKernelEvent::new(AgentKernelEventPayload::TurnMetrics(
        crate::TurnMetricsStreamData {
            metrics,
            usage,
            context: measured_context(result),
        },
    ))
}

fn track_tool_event(
    event: &AgentKernelEvent,
    calls: &mut HashSet<String>,
    results: &mut HashSet<String>,
) {
    match &event.payload {
        AgentKernelEventPayload::ToolCallStart(data) => {
            calls.insert(data.call_id.clone());
        }
        AgentKernelEventPayload::ToolResult(data) => {
            results.insert(data.call_id.clone());
        }
        AgentKernelEventPayload::UserInput(_)
        | AgentKernelEventPayload::ThinkingDelta(_)
        | AgentKernelEventPayload::AssistantTextStarted(_)
        | AgentKernelEventPayload::AssistantTextDelta(_)
        | AgentKernelEventPayload::AssistantTextCompleted(_)
        | AgentKernelEventPayload::AssistantTextCommitted(_)
        | AgentKernelEventPayload::ExecutionSummary(_)
        | AgentKernelEventPayload::TurnMetrics(_)
        | AgentKernelEventPayload::ToolArgsDelta(_)
        | AgentKernelEventPayload::ToolQueued(_)
        | AgentKernelEventPayload::ToolExecuting(_)
        | AgentKernelEventPayload::ToolProgressDelta(_)
        | AgentKernelEventPayload::PromptWarning(_)
        | AgentKernelEventPayload::UserActionRequest(_)
        | AgentKernelEventPayload::ContextCompaction(_) => {}
    }
}

fn missing_tool_events(
    result: &GeneralTurnResult,
    emitted_calls: &HashSet<String>,
    emitted_results: &HashSet<String>,
) -> Vec<AgentKernelEvent> {
    let mut events = Vec::new();
    for (index, item) in output_items(&result.outcome).iter().enumerate() {
        match item {
            KernelRunItem::ToolCall {
                call_id,
                name,
                arguments,
            } if !emitted_calls.contains(call_id) => {
                events.push(AgentKernelEvent::new(
                    AgentKernelEventPayload::ToolCallStart(ToolCallStartStreamData {
                        index,
                        call_id: call_id.clone(),
                        tool_name: name.clone(),
                        arguments: arguments.clone().into(),
                        round: 0,
                    }),
                ));
            }
            KernelRunItem::ToolResult {
                call_id,
                tool_name,
                content,
                is_error,
            } if !emitted_results.contains(call_id) => {
                events.push(AgentKernelEvent::new(AgentKernelEventPayload::ToolResult(
                    ToolResultStreamData {
                        call_id: call_id.clone(),
                        tool_name: tool_name.clone(),
                        content: content.clone(),
                        is_error: *is_error,
                    },
                )));
            }
            KernelRunItem::UserInput { .. }
            | KernelRunItem::Reasoning { .. }
            | KernelRunItem::ExecutionSummary { .. }
            | KernelRunItem::OutputText { .. }
            | KernelRunItem::ToolCall { .. }
            | KernelRunItem::ToolResult { .. } => {}
        }
    }
    events
}

fn output_items(outcome: &GeneralTurnOutcome) -> &[KernelRunItem] {
    match outcome {
        GeneralTurnOutcome::Completed { output_items, .. }
        | GeneralTurnOutcome::Waiting { output_items, .. } => output_items,
    }
}

#[cfg(test)]
#[path = "facade_tests.rs"]
mod tests;
