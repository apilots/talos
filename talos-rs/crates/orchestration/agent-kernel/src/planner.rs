use crate::{InvocationContext, LlmRequestEnvelope, Planner, PlannerKind, PlanningRequestEffect};

fn built_in_tool_guidance() -> String {
    "Use available tools whenever they can verify facts, inspect the workspace, execute commands, \
or fetch concrete results. Do not answer from memory when a tool can check the answer. If the user \
explicitly names a tool and it can satisfy the next verification step, honor that tool first. For \
codebase analysis, prefer Grep before Read, and use Glob first only when path \
shape or path type is the missing fact. Use Grep instead of shell grep or rg, Read \
instead of cat or head, and Glob instead of ls or find. Do not replace Grep, Read, \
or Glob with Bash when dedicated tools can answer directly. Use a direct answer only \
when no tool is needed."
        .to_string()
}

pub struct BuiltInPlanner;

impl BuiltInPlanner {
    pub fn new() -> Self {
        Self
    }
}

impl Planner for BuiltInPlanner {
    fn mode(&self) -> PlannerKind {
        PlannerKind::BuiltIn
    }

    fn build_request_effect(
        &self,
        _ctx: &InvocationContext,
        request: &LlmRequestEnvelope,
    ) -> PlanningRequestEffect {
        let reasoning_effort = if request
            .selected_model_info
            .as_ref()
            .map(|model| model.reasoning)
            .unwrap_or(false)
        {
            request
                .selected_model_info
                .as_ref()
                .map(|model| model.reasoning_effort)
        } else {
            None
        };
        let planning_instruction =
            (!request.tool_definitions.is_empty()).then(built_in_tool_guidance);
        PlanningRequestEffect {
            planning_instruction,
            reasoning_effort,
            clear_request_thoughts: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextStateBags, InvocationExecutionState, InvocationLimits, TurnPhase};
    use std::collections::HashMap;

    fn mock_ctx(input: &str) -> InvocationContext {
        InvocationContext {
            session_id: "s1".to_string(),
            message_id: "msg-1".to_string(),
            invocation_id: "i1".to_string(),
            agent_id: "a1".to_string(),
            workspace_id: "w1".to_string(),
            project_root: None,
            input: input.to_string(),
            selected_model_info: talos_types::ModelInfo {
                id: "m1".to_string(),
                name: "m1".to_string(),
                provider: "p1".to_string(),
                context_window: 0,
                reasoning: false,
                reasoning_effort: Default::default(),
                speed: Default::default(),
                input: Vec::new(),
                cost: Default::default(),
                compat: Default::default(),
            },
            phase: TurnPhase::Execution,
            planning_instruction: None,
            transfer_to: None,
            execution_state: InvocationExecutionState::default(),
            session: crate::InvocationSession {
                session_id: "s1".to_string(),
                ..crate::InvocationSession::default()
            },
            session_environment: None,
            state: HashMap::new(),
            state_bags: ContextStateBags::default(),
            limits: InvocationLimits::default(),
            events: Vec::new(),
            transcript_service: None,
            stream_tx: None,
        }
    }

    #[test]
    fn test_built_in_planner_mode() {
        let planner = BuiltInPlanner::new();
        assert_eq!(planner.mode(), PlannerKind::BuiltIn);
    }

    #[test]
    fn test_built_in_planner_gates_reasoning_effort_by_request_support() {
        let planner = BuiltInPlanner::new();
        let ctx = mock_ctx("hello");

        let mut supported_request = crate::LlmRequestEnvelope::default();
        supported_request.selected_model_info = Some(talos_types::ModelInfo {
            id: "m1".to_string(),
            name: "m1".to_string(),
            provider: "p1".to_string(),
            context_window: 0,
            reasoning: true,
            reasoning_effort: talos_types::ReasoningEffort::High,
            speed: Default::default(),
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        });
        let effect = planner
            .build_request_effect(&ctx, &supported_request)
            .reasoning_effort
            .expect("reasoning effort should be enabled");
        assert_eq!(effect, talos_types::ReasoningEffort::High);

        let unsupported_request = crate::LlmRequestEnvelope::default();
        assert!(planner
            .build_request_effect(&ctx, &unsupported_request)
            .reasoning_effort
            .is_none());
    }

    #[test]
    fn test_built_in_planner_adds_tool_guidance_only_when_tools_exist() {
        let planner = BuiltInPlanner::new();
        let ctx = mock_ctx("hello");
        let request = crate::LlmRequestEnvelope::default();
        assert!(planner
            .build_request_effect(&ctx, &request)
            .planning_instruction
            .is_none());

        let request_with_tools = crate::LlmRequestEnvelope {
            tool_definitions: vec![talos_llm_provider::ToolDefinition::new(
                "shell",
                "run shell",
                Some(serde_json::json!({"type":"object"})),
            )],
            ..crate::LlmRequestEnvelope::default()
        };
        let effect = planner.build_request_effect(&ctx, &request_with_tools);
        let instruction = effect
            .planning_instruction
            .expect("instruction should exist");
        assert!(instruction.contains("Use available tools"));
        assert!(instruction.contains("Do not answer from memory"));
        assert!(instruction.contains("honor that tool first"));
        assert!(instruction.contains("prefer Grep before Read"));
        assert!(instruction.contains("use Glob first only when path shape"));
        assert!(instruction.contains("instead of shell grep or rg"));
        assert!(instruction.contains("instead of cat or head"));
        assert!(instruction.contains("instead of ls or find"));
        assert!(instruction.contains("Use a direct answer only when no tool is needed"));
    }
}
