use crate::{
    run_general_turn, Agent, AgentRunResult, GeneralTurnContext, InvocationContext, Planner,
    TaskResult, TaskSpec, TaskStatus,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_llm_provider::LlmProvider;
use talos_memory::MemoryStore;
use talos_tools::ToolRegistry;
use talos_types::canonical::{CanonicalOutputItem, CanonicalUsage};

pub struct LlmKernelAgent {
    id: String,
    name: String,
    description: String,
    llm_provider: Arc<dyn LlmProvider>,
    memory_store: Arc<MemoryStore>,
    kernel_config: AgentKernelConfig,
    tool_registry: Arc<ToolRegistry>,
    planner: Arc<dyn Planner>,
}

impl LlmKernelAgent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        description: impl Into<String>,
        llm_provider: Arc<dyn LlmProvider>,
        memory_store: Arc<MemoryStore>,
        kernel_config: AgentKernelConfig,
        tool_registry: Arc<ToolRegistry>,
        planner: Arc<dyn Planner>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            llm_provider,
            memory_store,
            kernel_config,
            tool_registry,
            planner,
        }
    }

    async fn execute_planned_task(
        &self,
        ctx: &InvocationContext,
        task: &TaskSpec,
    ) -> TalosResult<TaskResult> {
        let tool_traces = Vec::new();
        let mut state_delta = HashMap::new();
        let mut error = None;
        let mut status = TaskStatus::Success;
        let mut output_items: Vec<CanonicalOutputItem> = Vec::new();

        let output = match run_general_turn(
            &GeneralTurnContext::builder()
                .session_id(&ctx.session_id)
                .agent_id(&self.id)
                .agent_name(&self.name)
                .agent_description(&self.description)
                .input(&task.goal)
                .selected_model(&ctx.model)
                .llm_provider(self.llm_provider.clone())
                .tool_registry(self.tool_registry.clone())
                .memory_store(self.memory_store.clone())
                .kernel_config(self.kernel_config.clone())
                .metadata(ctx.metadata.clone())
                .stream_tx(ctx.stream_tx.clone())
                .build(),
        )
        .await
        {
            Ok(general_result) => {
                output_items = general_result.output_items;
                general_result.output_text
            }
            Err(e) => {
                status = TaskStatus::Failed;
                error = Some(e.to_string());
                output_items.push(CanonicalOutputItem::Reasoning {
                    text: "Kernel summary: general task failed during kernel orchestration."
                        .to_string(),
                });
                format!("Task failed: {}", e)
            }
        };

        if !output_items
            .iter()
            .any(|item| matches!(item, CanonicalOutputItem::OutputText { .. }))
        {
            output_items.push(CanonicalOutputItem::OutputText {
                text: output.clone(),
            });
        }

        state_delta.insert("last_task_kind".to_string(), json!(task.kind));
        state_delta.insert("last_task_id".to_string(), json!(task.id));

        Ok(TaskResult {
            task_id: task.id.clone(),
            kind: task.kind.clone(),
            goal: task.goal.clone(),
            status,
            output,
            output_items,
            tool_traces,
            state_delta,
            error,
        })
    }

    fn inject_planning_instruction_metadata(
        ctx: &mut InvocationContext,
        instruction: impl Into<String>,
    ) {
        let mut metadata = match ctx.metadata.take() {
            Some(Value::Object(map)) => map,
            Some(other) => {
                let mut map = serde_json::Map::new();
                map.insert("request_metadata".to_string(), other);
                map
            }
            None => serde_json::Map::new(),
        };
        metadata.insert(
            "planning_instruction".to_string(),
            json!(instruction.into()),
        );
        ctx.metadata = Some(Value::Object(metadata));
    }

    fn metadata_str(metadata: Option<&Value>, key: &str) -> Option<String> {
        metadata
            .and_then(|value| value.get(key))
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
    }

    fn maybe_request_transfer(ctx: &mut InvocationContext) -> Option<String> {
        if ctx
            .state_bags
            .request_state
            .get("kernel_transfer_consumed")
            .is_some()
        {
            return None;
        }
        let transfer_to = Self::metadata_str(ctx.metadata.as_ref(), "kernel_transfer_to")?;
        if transfer_to.trim().is_empty() {
            return None;
        }
        ctx.set_request_state("kernel_transfer_consumed", json!(true));
        Some(transfer_to)
    }
}

#[async_trait]
impl Agent for LlmKernelAgent {
    fn id(&self) -> &str {
        &self.id
    }

    async fn run(&self, ctx: &mut InvocationContext) -> TalosResult<AgentRunResult> {
        if let Some(instruction) = self.planner.build_planning_instruction(ctx) {
            ctx.set_request_state(
                "planning_instruction".to_string(),
                json!(instruction.clone()),
            );
            Self::inject_planning_instruction_metadata(ctx, instruction);
        }

        let plan = self.planner.plan(ctx).await?;
        let mut task_results = Vec::new();
        let mut merged_state_delta = HashMap::new();

        for task in plan
            .tasks
            .iter()
            .take(self.kernel_config.max_task_per_turn.max(1))
        {
            let result = self.execute_planned_task(ctx, task).await?;
            for (k, v) in &result.state_delta {
                merged_state_delta.insert(k.clone(), v.clone());
            }
            task_results.push(result);
        }

        let output_text = task_results
            .last()
            .map(|result| result.output.clone())
            .unwrap_or_else(|| "No task produced output".to_string());
        let output_items = task_results
            .iter()
            .flat_map(|result| result.output_items.clone())
            .collect::<Vec<_>>();

        let mut metadata = HashMap::new();
        metadata.insert("planned_task_count".to_string(), json!(plan.tasks.len()));
        metadata.insert("executed_task_count".to_string(), json!(task_results.len()));
        metadata.insert(
            "max_tool_rounds".to_string(),
            json!(self.kernel_config.max_tool_rounds),
        );
        metadata.insert(
            "tool_output_max_bytes".to_string(),
            json!(self.kernel_config.tool_result_max_bytes),
        );
        metadata.insert(
            "context_compaction_max_bytes".to_string(),
            json!(self.kernel_config.context_compaction_max_bytes),
        );
        if let Some(processed) = self.planner.process_planning_response(ctx, &output_text) {
            ctx.set_request_state("planning_response".to_string(), json!(processed));
            metadata.insert("planning_response".to_string(), json!(processed));
        }

        Ok(AgentRunResult {
            output_text,
            output_items,
            stop_reason: "end_turn".to_string(),
            usage: CanonicalUsage::default(),
            task_results,
            state_delta: merged_state_delta,
            continue_loop: false,
            requested_transfer: Self::maybe_request_transfer(ctx),
            metadata,
        })
    }
}
