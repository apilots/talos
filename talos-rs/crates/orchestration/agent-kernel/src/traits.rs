use crate::{
    AgentRunResult, ExecutionPath, InvocationContext, PlannerKind, PlanningResponseEffect,
    RunRequest, RunResponse, TaskPlan, TaskResult, TaskSpec, ThinkingMode,
};
use async_trait::async_trait;
use std::sync::Arc;
use talos_error::TalosResult;

#[async_trait]
pub trait Agent: Send + Sync {
    fn id(&self) -> &str;
    async fn run(&self, ctx: &mut InvocationContext) -> TalosResult<AgentRunResult>;
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

    fn build_planning_instruction(&self, _ctx: &InvocationContext) -> Option<String> {
        None
    }

    fn process_planning_response_effect(
        &self,
        ctx: &InvocationContext,
        response_text: &str,
    ) -> PlanningResponseEffect {
        PlanningResponseEffect {
            normalized_text: self.process_planning_response(ctx, response_text),
            state_delta: Vec::new(),
            branch_override: None,
        }
    }

    fn process_planning_response(
        &self,
        _ctx: &InvocationContext,
        _response_text: &str,
    ) -> Option<String> {
        None
    }

    async fn plan(&self, ctx: &InvocationContext) -> TalosResult<TaskPlan>;
}

#[async_trait]
pub trait TaskHandler: Send + Sync {
    fn supports(&self, task: &TaskSpec) -> bool;
    async fn handle(&self, ctx: &mut InvocationContext, task: &TaskSpec)
        -> TalosResult<TaskResult>;
}

#[async_trait]
pub trait Plugin: Send + Sync {
    fn name(&self) -> &str;

    async fn before_plan(&self, _ctx: &mut InvocationContext) -> TalosResult<()> {
        Ok(())
    }

    async fn after_plan(&self, _ctx: &mut InvocationContext, _plan: &TaskPlan) -> TalosResult<()> {
        Ok(())
    }

    async fn before_task(&self, _ctx: &mut InvocationContext, _task: &TaskSpec) -> TalosResult<()> {
        Ok(())
    }

    async fn after_task(
        &self,
        _ctx: &mut InvocationContext,
        _result: &TaskResult,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn after_finalize(
        &self,
        _ctx: &mut InvocationContext,
        _result: &mut AgentRunResult,
    ) -> TalosResult<()> {
        Ok(())
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
