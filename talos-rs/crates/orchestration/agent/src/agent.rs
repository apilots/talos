use async_trait::async_trait;
use chrono::Utc;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use uuid::Uuid;

mod kernel;
mod mock;
mod request;
mod session;
mod stream;
#[cfg(test)]
mod tests;
mod tooling;

pub use mock::MockAgent;

use crate::{
    flush_manager::MemoryFlushManager,
    service::{
        OrchestratorMessageCompletedEvent, OrchestratorMessageErrorEvent,
        OrchestratorMessageStartedEvent,
    },
    Agent, AgentOrchestrator, AgentResponse, OrchestratorEvent, OrchestratorExecution,
    OrchestratorRequest,
};
use talos_agent_kernel::{KernelRuntime, RunRequest};
use talos_config::{AgentKernelConfig, PlannerMode};
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::LlmProvider;
use talos_memory::MemoryStore;
use talos_tools::ToolRegistry;
use talos_types::talos::{StopReason as TalosStopReason, TalosUsage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KernelExecutionMode {
    Single,
    Sequential,
    Parallel,
    Loop,
}

#[derive(Clone)]
pub(crate) struct ActiveTurnHandle {
    execution_task: AbortHandle,
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

    pub fn set_prompt_budget(&mut self, max_static_tokens: usize, max_dynamic_tokens: usize) {
        self.kernel_config.prompt.max_static_tokens = max_static_tokens.max(1);
        self.kernel_config.prompt.max_dynamic_tokens = max_dynamic_tokens.max(1);
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
        let agent_request = self.to_agent_request(request);
        let input = Self::extract_text_input(&agent_request.input);
        let invocation_id = format!("inv-{}", Utc::now().timestamp_millis());
        let session_id = agent_request.session.context.session_id.clone();
        let selected_model_info = self.resolve_selected_model_info(&agent_request.session);
        let phase = agent_request.phase.clone();
        let planning_instruction = agent_request.planning_instruction.clone();
        let transfer_to = agent_request.transfer_to.clone();
        let kernel_agent = self.build_kernel_agent_for_request()?;
        let runtime = self.kernel_runtime.clone();
        let agent_id = self.id.clone();
        let kernel_config = self.kernel_config.clone();
        let run_session_id = session_id.clone();
        let run_handle = tokio::spawn({
            async move {
                runtime
                    .run_turn(
                        kernel_agent,
                        RunRequest {
                            session_id: run_session_id,
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
        self.register_active_turn(
            &session_id,
            ActiveTurnHandle {
                execution_task: run_handle.abort_handle(),
            },
        );
        let result = match run_handle.await {
            Ok(Ok(result)) => result,
            Ok(Err(err)) => {
                self.clear_active_turn(&session_id);
                return Err(err);
            }
            Err(err) if err.is_cancelled() => {
                self.clear_active_turn(&session_id);
                return Ok(OrchestratorExecution {
                    output_text: String::new(),
                    output_items: Vec::new(),
                    reasoning: Vec::new(),
                    stop_reason: TalosStopReason::Cancelled,
                    usage: TalosUsage::default(),
                    task_count: 0,
                    session_id,
                });
            }
            Err(err) => {
                self.clear_active_turn(&session_id);
                return Err(TalosError::InternalError(format!(
                    "kernel task join failed: {}",
                    err
                )));
            }
        };
        self.clear_active_turn(&session_id);
        let reasoning = Self::collect_reasoning(&result.output_items);
        Ok(OrchestratorExecution {
            output_text: result.output_text,
            output_items: result.output_items,
            reasoning,
            stop_reason: kernel::map_stop_reason(result.stop_reason),
            usage: result.usage,
            task_count: result.task_results.len() as u32,
            session_id,
        })
    }

    async fn run_stream(
        &self,
        request: OrchestratorRequest,
    ) -> TalosResult<mpsc::Receiver<OrchestratorEvent>> {
        let (tx, rx) = mpsc::channel(256);
        let this = self.clone();
        let stream_task = tokio::spawn(async move {
            let agent_request = this.to_agent_request(request);
            let input = Self::extract_text_input(&agent_request.input);
            let request_id = format!("req_{}", Uuid::new_v4().simple());
            let invocation_id = format!("inv-{}", Utc::now().timestamp_millis());
            let session_id = agent_request.session.context.session_id.clone();
            let model = if agent_request.session.config.llm.model.is_empty() {
                this.model.clone()
            } else {
                agent_request.session.config.llm.model.clone()
            };
            let selected_model_info = this.resolve_selected_model_info(&agent_request.session);
            let phase = agent_request.phase.clone();
            let planning_instruction = agent_request.planning_instruction.clone();
            let transfer_to = agent_request.transfer_to.clone();
            let kernel_agent = match this.build_kernel_agent_for_request() {
                Ok(agent) => agent,
                Err(err) => {
                    let _ = tx
                        .send(OrchestratorEvent::MessageError(
                            OrchestratorMessageErrorEvent {
                                request_id: request_id.clone(),
                                error: err.to_string(),
                            },
                        ))
                        .await;
                    return;
                }
            };
            let (kernel_tx, mut kernel_rx) = mpsc::channel(256);
            let runtime = this.kernel_runtime.clone();
            let kernel_input = RunRequest {
                session_id: agent_request.session.context.session_id.clone(),
                invocation_id,
                agent_id: this.id.clone(),
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
                kernel_config: Some(this.kernel_config.clone()),
                stream_tx: Some(kernel_tx),
            };

            let _ = tx
                .send(OrchestratorEvent::MessageStarted(
                    OrchestratorMessageStartedEvent {
                        request_id: request_id.clone(),
                        model,
                        usage: TalosUsage::default(),
                        iteration: 0,
                        step_id: "start".to_string(),
                    },
                ))
                .await;

            let run_handle =
                tokio::spawn(async move { runtime.run_turn(kernel_agent, kernel_input).await });
            this.register_active_turn(
                &session_id,
                ActiveTurnHandle {
                    execution_task: run_handle.abort_handle(),
                },
            );

            let mut item_index = 0_u64;
            while let Some(kernel_event) = kernel_rx.recv().await {
                if let Some(event) =
                    Self::map_kernel_stream_event(kernel_event, &request_id, &mut item_index)
                {
                    let _ = tx.send(event).await;
                }
            }

            match run_handle.await {
                Ok(Ok(result)) => {
                    let _ = tx
                        .send(OrchestratorEvent::MessageCompleted(
                            OrchestratorMessageCompletedEvent {
                                request_id: request_id.clone(),
                                stop_reason: kernel::map_stop_reason(result.stop_reason),
                                usage: result.usage,
                                total_output_items: item_index,
                                iteration: item_index.saturating_add(1),
                                step_id: "completed".to_string(),
                                task_count: result.task_results.len() as u32,
                            },
                        ))
                        .await;
                }
                Err(err) if err.is_cancelled() => {
                    let _ = tx
                        .send(OrchestratorEvent::MessageCompleted(
                            OrchestratorMessageCompletedEvent {
                                request_id: request_id.clone(),
                                stop_reason: TalosStopReason::Cancelled,
                                usage: TalosUsage::default(),
                                total_output_items: item_index,
                                iteration: item_index.saturating_add(1),
                                step_id: "cancelled".to_string(),
                                task_count: 0,
                            },
                        ))
                        .await;
                }
                Ok(Err(err)) => {
                    let _ = tx
                        .send(OrchestratorEvent::MessageError(
                            OrchestratorMessageErrorEvent {
                                request_id: request_id.clone(),
                                error: err.to_string(),
                            },
                        ))
                        .await;
                }
                Err(err) => {
                    let _ = tx
                        .send(OrchestratorEvent::MessageError(
                            OrchestratorMessageErrorEvent {
                                request_id: request_id.clone(),
                                error: format!("kernel stream task join failed: {}", err),
                            },
                        ))
                        .await;
                }
            }
            this.clear_active_turn(&session_id);
        });
        drop(stream_task);
        Ok(rx)
    }

    async fn close_session(&self, session: talos_types::TalosSession) -> TalosResult<()> {
        self.close_session_impl(session).await
    }

    async fn drain_memory_flushes(&self, timeout: Duration) -> TalosResult<()> {
        self.drain_pending_memory_flushes(timeout).await
    }

    fn pending_memory_flushes(&self) -> usize {
        self.pending_memory_flush_count()
    }

    async fn cancel_active_turn(&self, session_id: &str) -> TalosResult<()> {
        let Some((_, handle)) = self.active_turns.remove(session_id) else {
            return Ok(());
        };
        handle.execution_task.abort();
        Ok(())
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
