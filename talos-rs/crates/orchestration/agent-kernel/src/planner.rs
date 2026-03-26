use crate::{InvocationContext, Planner, TaskConstraints, TaskKind, TaskPlan, TaskSpec};
use async_trait::async_trait;
use serde_json::json;
use talos_error::TalosResult;
use uuid::Uuid;

const PLAN_REACT_INSTRUCTION: &str = "Follow a Plan->Reason->Act loop. First produce a concise \
plan, then use tools when needed, then return the final answer.";

fn build_single_task_plan(ctx: &InvocationContext) -> TaskPlan {
    let task = TaskSpec {
        id: Uuid::new_v4().to_string(),
        kind: TaskKind::General,
        goal: ctx.input.clone(),
        input: json!({ "raw_input": ctx.input }),
        constraints: TaskConstraints::default(),
        tool_candidates: Vec::new(),
        expected_output: None,
    };

    TaskPlan {
        tasks: vec![task],
    }
}

pub struct BuiltInPlanner;

impl BuiltInPlanner {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Planner for BuiltInPlanner {
    async fn plan(&self, ctx: &InvocationContext) -> TalosResult<TaskPlan> {
        Ok(build_single_task_plan(ctx))
    }
}

pub struct PlanReActPlanner;

impl PlanReActPlanner {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Planner for PlanReActPlanner {
    fn build_planning_instruction(&self, _ctx: &InvocationContext) -> Option<String> {
        Some(PLAN_REACT_INSTRUCTION.to_string())
    }

    fn process_planning_response(
        &self,
        _ctx: &InvocationContext,
        response_text: &str,
    ) -> Option<String> {
        let normalized = response_text.trim();
        if normalized.is_empty() {
            None
        } else {
            Some(normalized.to_string())
        }
    }

    async fn plan(&self, ctx: &InvocationContext) -> TalosResult<TaskPlan> {
        Ok(build_single_task_plan(ctx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextStateBags, InvocationLimits, TurnPhase};
    use std::collections::HashMap;

    fn mock_ctx(input: &str, _context_hint: Option<serde_json::Value>) -> InvocationContext {
        InvocationContext {
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            agent_id: "a1".to_string(),
            workspace_id: "w1".to_string(),
            input: input.to_string(),
            model: "m1".to_string(),
            phase: TurnPhase::Execution,
            planning_instruction: None,
            transfer_to: None,
            state: HashMap::new(),
            state_bags: ContextStateBags::default(),
            limits: InvocationLimits::default(),
            events: Vec::new(),
            stream_tx: None,
        }
    }

    #[tokio::test]
    async fn test_built_in_planner_defaults_to_general_without_direct_tool_mode() {
        let planner = BuiltInPlanner::new();
        let ctx = mock_ctx("run ls", None);
        let plan = planner.plan(&ctx).await.expect("plan should succeed");
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].kind, TaskKind::General);
        assert!(plan.tasks[0].tool_candidates.is_empty());
    }

    #[tokio::test]
    async fn test_built_in_planner_ignores_direct_tool_mode_context_hint() {
        let planner = BuiltInPlanner::new();
        let ctx = mock_ctx("run ls", Some(json!({"direct_tool_mode": true})));
        let plan = planner.plan(&ctx).await.expect("plan should succeed");
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].kind, TaskKind::General);
        assert!(plan.tasks[0].tool_candidates.is_empty());
    }

    #[test]
    fn test_plan_react_planner_instruction_and_response_processing() {
        let planner = PlanReActPlanner::new();
        let ctx = mock_ctx("hello", None);
        let instruction = planner
            .build_planning_instruction(&ctx)
            .expect("instruction should exist");
        assert!(instruction.contains("Plan->Reason->Act"));

        assert_eq!(planner.process_planning_response(&ctx, "   "), None);
        assert_eq!(
            planner.process_planning_response(&ctx, "  final answer "),
            Some("final answer".to_string())
        );
    }
}
