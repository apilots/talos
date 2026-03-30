use crate::{
    AgentRunResult, CallbackControl, ExecutionEvent, ExecutionEventHandler, ExecutionPath,
    InvocationContext, KernelExecutionSettings, LlmRequestEnvelope, LlmResponseEnvelope,
    PlannerKind, PlanningResponseEffect, RunRequest, RunResponse, TaskResult, ThinkingMode,
    ToolCallbackPayload,
};
use async_trait::async_trait;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_llm_provider::NativeThinkingConfig;

#[async_trait]
pub trait Agent: Send + Sync {
    fn id(&self) -> &str;
    async fn run(
        &self,
        ctx: &mut InvocationContext,
        event_handler: Option<&mut dyn ExecutionEventHandler>,
    ) -> TalosResult<AgentRunResult>;
}

pub trait BaseKernelAgent: Agent {}

impl<T: Agent + ?Sized> BaseKernelAgent for T {}

#[async_trait]
pub trait Planner: Send + Sync {
    fn kind(&self) -> PlannerKind {
        PlannerKind::BuiltIn
    }

    fn thinking_mode(&self) -> ThinkingMode {
        ThinkingMode::Disabled
    }

    fn default_execution_path(&self) -> ExecutionPath {
        ExecutionPath::ReactiveLoop
    }

    fn native_thinking_config(
        &self,
        _ctx: &InvocationContext,
        _request: &LlmRequestEnvelope,
    ) -> Option<NativeThinkingConfig> {
        None
    }

    fn build_planning_instruction(
        &self,
        _ctx: &InvocationContext,
        _request: &LlmRequestEnvelope,
    ) -> Option<String> {
        None
    }

    fn process_planning_response_effect(
        &self,
        ctx: &InvocationContext,
        response: &LlmResponseEnvelope,
    ) -> PlanningResponseEffect {
        PlanningResponseEffect {
            normalized_text: self.process_planning_response(ctx, response),
            segments: Vec::new(),
            output_text_override: None,
            clear_output_text: false,
            state_delta: Vec::new(),
            branch_override: None,
        }
    }

    fn process_planning_response(
        &self,
        _ctx: &InvocationContext,
        _response: &LlmResponseEnvelope,
    ) -> Option<String> {
        None
    }
}

#[async_trait]
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;

    async fn before_agent_run(&self, _ctx: &mut InvocationContext) -> TalosResult<()> {
        Ok(())
    }

    async fn before_run(&self, _ctx: &mut InvocationContext) -> TalosResult<()> {
        Ok(())
    }

    async fn after_agent_run(
        &self,
        _ctx: &mut InvocationContext,
        _result: &mut AgentRunResult,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn on_event(
        &self,
        _ctx: &mut InvocationContext,
        _event: &mut ExecutionEvent,
    ) -> TalosResult<bool> {
        Ok(true)
    }

    async fn after_run(&self, _ctx: &mut InvocationContext) -> TalosResult<()> {
        Ok(())
    }

    async fn before_flow(
        &self,
        _ctx: &mut InvocationContext,
        _settings: &KernelExecutionSettings,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn after_flow(
        &self,
        _ctx: &mut InvocationContext,
        _result: &TaskResult,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn before_model(
        &self,
        _ctx: &mut InvocationContext,
        _request: &mut LlmRequestEnvelope,
    ) -> TalosResult<CallbackControl> {
        Ok(CallbackControl::Continue)
    }

    async fn after_model(
        &self,
        _ctx: &mut InvocationContext,
        _response: &mut LlmResponseEnvelope,
    ) -> TalosResult<CallbackControl> {
        Ok(CallbackControl::Continue)
    }

    async fn before_tool(
        &self,
        _ctx: &mut InvocationContext,
        _payload: &ToolCallbackPayload,
    ) -> TalosResult<CallbackControl> {
        Ok(CallbackControl::Continue)
    }

    async fn after_tool(
        &self,
        _ctx: &mut InvocationContext,
        _payload: &ToolCallbackPayload,
    ) -> TalosResult<CallbackControl> {
        Ok(CallbackControl::Continue)
    }
}

#[async_trait]
pub trait Runner: Send + Sync {
    async fn run_turn(
        &self,
        agent: Arc<dyn Agent>,
        request: RunRequest,
    ) -> TalosResult<RunResponse>;
}
