use crate::{
    execution::{PlanningResponseSegment, PlanningResponseSegmentKind},
    ExecutionPath, InvocationContext, LlmRequestEnvelope, LlmResponseEnvelope, Planner,
    PlannerKind, PlanningResponseEffect, ThinkingMode,
};
use talos_llm_provider::{NativeThinkingConfig, NativeThinkingEffort};

const PLAN_REACT_PLANNING_TAG: &str = "/*PLANNING*/";
const PLAN_REACT_REPLANNING_TAG: &str = "/*REPLANNING*/";
const PLAN_REACT_REASONING_TAG: &str = "/*REASONING*/";
const PLAN_REACT_ACTION_TAG: &str = "/*ACTION*/";
const PLAN_REACT_FINAL_ANSWER_TAG: &str = "/*FINAL_ANSWER*/";

const PLAN_REACT_INSTRUCTION: &str = "When answering, prefer available tools over memorized \
knowledge.\n\nFollow this process:\n1. Start with a natural-language plan.\n2. Use tools to \
execute the plan and interleave tool use with reasoning updates.\n3. End with one final answer.\n\n\
Use this format:\n- planning under /*PLANNING*/\n- replanning under /*REPLANNING*/ when needed\n\
- reasoning under /*REASONING*/\n- tool actions under /*ACTION*/\n- final answer under \
/*FINAL_ANSWER*/.\n\nThe plan should be coherent, tool-grounded, and decomposed into numbered \
steps. If execution fails, revise the plan under /*REPLANNING*/. The final answer must be precise \
and explicitly state when the task cannot be completed with the available context.";

const BUILT_IN_TOOL_GUIDANCE: &str = "Use available tools whenever they can verify facts, inspect \
the workspace, execute commands, or fetch concrete results. Do not answer from memory when a tool \
can check the answer. Use a direct answer only when no tool is needed.";

pub struct BuiltInPlanner;

impl BuiltInPlanner {
    pub fn new() -> Self {
        Self
    }
}

impl Planner for BuiltInPlanner {
    fn kind(&self) -> PlannerKind {
        PlannerKind::BuiltIn
    }

    fn thinking_mode(&self) -> ThinkingMode {
        ThinkingMode::ModelNative
    }

    fn default_execution_path(&self) -> ExecutionPath {
        ExecutionPath::DirectAnswer
    }

    fn build_planning_instruction(
        &self,
        _ctx: &InvocationContext,
        request: &LlmRequestEnvelope,
    ) -> Option<String> {
        (!request.tool_definitions.is_empty()).then(|| BUILT_IN_TOOL_GUIDANCE.to_string())
    }

    fn native_thinking_config(
        &self,
        _ctx: &InvocationContext,
        request: &LlmRequestEnvelope,
    ) -> Option<NativeThinkingConfig> {
        if !request.native_thinking_support.is_supported() {
            return None;
        }
        Some(NativeThinkingConfig {
            effort: NativeThinkingEffort::Medium,
            budget_tokens: Some(2048),
        })
    }
}

pub struct PlanReActPlanner;

impl PlanReActPlanner {
    pub fn new() -> Self {
        Self
    }
}

impl Planner for PlanReActPlanner {
    fn kind(&self) -> PlannerKind {
        PlannerKind::PlanReAct
    }

    fn thinking_mode(&self) -> ThinkingMode {
        ThinkingMode::PlanningProcessor
    }

    fn default_execution_path(&self) -> ExecutionPath {
        ExecutionPath::ReactiveLoop
    }

    fn build_planning_instruction(
        &self,
        _ctx: &InvocationContext,
        _request: &LlmRequestEnvelope,
    ) -> Option<String> {
        Some(PLAN_REACT_INSTRUCTION.to_string())
    }

    fn process_planning_response_effect(
        &self,
        _ctx: &InvocationContext,
        response: &LlmResponseEnvelope,
    ) -> PlanningResponseEffect {
        let normalized = response.output_text.trim();
        if normalized.is_empty() {
            return PlanningResponseEffect::default();
        }

        let mut segments = Vec::new();
        let mut final_output_text = None;
        let mut saw_tagged_thought = false;

        if let Some((reasoning_text, final_answer_text)) =
            split_by_last_pattern(normalized, PLAN_REACT_FINAL_ANSWER_TAG)
        {
            if !reasoning_text.trim().is_empty() {
                segments.push(PlanningResponseSegment {
                    kind: PlanningResponseSegmentKind::Thought,
                    text: reasoning_text.trim().to_string(),
                });
                saw_tagged_thought = true;
            }
            if !final_answer_text.trim().is_empty() {
                segments.push(PlanningResponseSegment {
                    kind: PlanningResponseSegmentKind::FinalAnswer,
                    text: final_answer_text.trim().to_string(),
                });
                final_output_text = Some(final_answer_text.trim().to_string());
            }
        } else if starts_with_tagged_thought(normalized) {
            segments.push(PlanningResponseSegment {
                kind: PlanningResponseSegmentKind::Thought,
                text: normalized.to_string(),
            });
            saw_tagged_thought = true;
        }

        PlanningResponseEffect {
            normalized_text: Some(normalized.to_string()),
            segments,
            output_text_override: final_output_text,
            clear_output_text: saw_tagged_thought,
            state_delta: Vec::new(),
            branch_override: None,
        }
    }
}

fn split_by_last_pattern<'a>(text: &'a str, separator: &str) -> Option<(&'a str, &'a str)> {
    let index = text.rfind(separator)?;
    Some((
        &text[..index + separator.len()],
        &text[index + separator.len()..],
    ))
}

fn starts_with_tagged_thought(text: &str) -> bool {
    [
        PLAN_REACT_PLANNING_TAG,
        PLAN_REACT_REPLANNING_TAG,
        PLAN_REACT_REASONING_TAG,
        PLAN_REACT_ACTION_TAG,
    ]
    .iter()
    .any(|tag| text.starts_with(tag))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextStateBags, InvocationExecutionState, InvocationLimits, TurnPhase};
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
            execution_state: InvocationExecutionState::default(),
            session: crate::InvocationSession {
                session_id: "s1".to_string(),
                ..crate::InvocationSession::default()
            },
            state: HashMap::new(),
            state_bags: ContextStateBags::default(),
            limits: InvocationLimits::default(),
            events: Vec::new(),
            plugin_manager: crate::default_plugin_manager(),
            session_service: None,
            stream_tx: None,
        }
    }

    #[test]
    fn test_built_in_planner_defaults_to_direct_answer_model_thinking() {
        let planner = BuiltInPlanner::new();
        assert_eq!(planner.kind(), PlannerKind::BuiltIn);
        assert_eq!(planner.thinking_mode(), ThinkingMode::ModelNative);
        assert_eq!(
            planner.default_execution_path(),
            ExecutionPath::DirectAnswer
        );
    }

    #[test]
    fn test_built_in_planner_gates_native_thinking_by_request_support() {
        let planner = BuiltInPlanner::new();
        let ctx = mock_ctx("hello", None);

        let mut supported_request = crate::LlmRequestEnvelope::default();
        supported_request.native_thinking_support =
            talos_llm_provider::NativeThinkingSupport::Supported;
        assert!(planner
            .native_thinking_config(&ctx, &supported_request)
            .is_some());

        let unsupported_request = crate::LlmRequestEnvelope::default();
        assert!(planner
            .native_thinking_config(&ctx, &unsupported_request)
            .is_none());
    }

    #[test]
    fn test_built_in_planner_adds_tool_guidance_only_when_tools_exist() {
        let planner = BuiltInPlanner::new();
        let ctx = mock_ctx("hello", None);
        let request = crate::LlmRequestEnvelope::default();
        assert!(planner.build_planning_instruction(&ctx, &request).is_none());

        let request_with_tools = crate::LlmRequestEnvelope {
            tool_definitions: vec![talos_llm_provider::ToolDefinition::new(
                "shell",
                "run shell",
                Some(serde_json::json!({"type":"object"})),
            )],
            ..crate::LlmRequestEnvelope::default()
        };
        let instruction = planner
            .build_planning_instruction(&ctx, &request_with_tools)
            .expect("instruction should exist");
        assert!(instruction.contains("Use available tools"));
    }

    #[test]
    fn test_plan_react_planner_instruction_and_response_processing() {
        let planner = PlanReActPlanner::new();
        let ctx = mock_ctx("hello", None);
        let request = crate::LlmRequestEnvelope::default();
        let instruction = planner
            .build_planning_instruction(&ctx, &request)
            .expect("instruction should exist");
        assert!(instruction.contains("/*PLANNING*/"));
        assert!(instruction.contains("/*FINAL_ANSWER*/"));

        let empty_response = crate::LlmResponseEnvelope {
            output_text: "   ".to_string(),
            ..crate::LlmResponseEnvelope::default()
        };
        assert_eq!(
            planner.process_planning_response(&ctx, &empty_response),
            None
        );

        let final_response = crate::LlmResponseEnvelope {
            output_text: "/*REASONING*/ think\n/*FINAL_ANSWER*/done".to_string(),
            ..crate::LlmResponseEnvelope::default()
        };
        let effect = planner.process_planning_response_effect(&ctx, &final_response);
        assert_eq!(effect.output_text_override.as_deref(), Some("done"));
        assert!(effect.clear_output_text);
        assert_eq!(effect.segments.len(), 2);
        assert!(matches!(
            effect.segments[0].kind,
            PlanningResponseSegmentKind::Thought
        ));
        assert!(matches!(
            effect.segments[1].kind,
            PlanningResponseSegmentKind::FinalAnswer
        ));
    }
}
