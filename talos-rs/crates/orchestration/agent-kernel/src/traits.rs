use crate::{
    AgentRunResult, CallbackControl, ExecutionEvent, ExecutionEventHandler,
    InvocationContext, KernelExecutionSettings, LlmRequestEnvelope, LlmResponseEnvelope,
    PlannerKind, PlanningRequestEffect, PlanningResponseEffect, RunRequest, RunResponse, TaskResult,
    ToolCallbackPayload,
};
use async_trait::async_trait;
use std::sync::Arc;
use talos_error::TalosResult;

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
    fn mode(&self) -> PlannerKind {
        PlannerKind::BuiltIn
    }

    /// Produce the request-phase effect: planning instruction and optional native thinking config.
    fn build_request_effect(
        &self,
        _ctx: &InvocationContext,
        _request: &LlmRequestEnvelope,
    ) -> PlanningRequestEffect {
        PlanningRequestEffect::default()
    }

    /// Produce the response-phase effect: normalized text, segments, final answer detection, etc.
    fn process_response_effect(
        &self,
        _ctx: &InvocationContext,
        _response: &LlmResponseEnvelope,
    ) -> PlanningResponseEffect {
        PlanningResponseEffect::default()
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
