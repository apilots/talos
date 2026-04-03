use crate::{
    execution::{PlanningResponseSegment, PlanningResponseSegmentKind},
    InvocationContext, LlmRequestEnvelope, LlmResponseEnvelope, Planner, PlannerKind,
    PlanningRequestEffect, PlanningResponseEffect,
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
    fn mode(&self) -> PlannerKind {
        PlannerKind::BuiltIn
    }

    fn build_request_effect(
        &self,
        _ctx: &InvocationContext,
        request: &LlmRequestEnvelope,
    ) -> PlanningRequestEffect {
        let native_thinking = if request.native_thinking_support.is_supported() {
            Some(NativeThinkingConfig {
                effort: NativeThinkingEffort::Medium,
                budget_tokens: Some(2048),
            })
        } else {
            None
        };
        let planning_instruction =
            (!request.tool_definitions.is_empty()).then(|| BUILT_IN_TOOL_GUIDANCE.to_string());
        PlanningRequestEffect {
            planning_instruction,
            native_thinking,
            clear_request_thoughts: false,
        }
    }
}

pub struct PlanReActPlanner;

impl PlanReActPlanner {
    pub fn new() -> Self {
        Self
    }
}

impl Planner for PlanReActPlanner {
    fn mode(&self) -> PlannerKind {
        PlannerKind::PlanReAct
    }

    fn build_request_effect(
        &self,
        _ctx: &InvocationContext,
        _request: &LlmRequestEnvelope,
    ) -> PlanningRequestEffect {
        PlanningRequestEffect {
            planning_instruction: Some(PLAN_REACT_INSTRUCTION.to_string()),
            native_thinking: None,
            clear_request_thoughts: false,
        }
    }

    fn process_response_effect(
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
        let mut has_final_answer = false;

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
                has_final_answer = true;
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
            has_final_answer,
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

    fn mock_ctx(input: &str) -> InvocationContext {
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
    fn test_built_in_planner_mode() {
        let planner = BuiltInPlanner::new();
        assert_eq!(planner.mode(), PlannerKind::BuiltIn);
    }

    #[test]
    fn test_built_in_planner_gates_native_thinking_by_request_support() {
        let planner = BuiltInPlanner::new();
        let ctx = mock_ctx("hello");

        let mut supported_request = crate::LlmRequestEnvelope::default();
        supported_request.native_thinking_support =
            talos_llm_provider::NativeThinkingSupport::Supported;
        assert!(planner
            .build_request_effect(&ctx, &supported_request)
            .native_thinking
            .is_some());

        let unsupported_request = crate::LlmRequestEnvelope::default();
        assert!(planner
            .build_request_effect(&ctx, &unsupported_request)
            .native_thinking
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
    }

    #[test]
    fn test_plan_react_planner_instruction_and_response_processing() {
        let planner = PlanReActPlanner::new();
        let ctx = mock_ctx("hello");
        let request = crate::LlmRequestEnvelope::default();
        let effect = planner.build_request_effect(&ctx, &request);
        let instruction = effect
            .planning_instruction
            .expect("instruction should exist");
        assert!(instruction.contains("/*PLANNING*/"));
        assert!(instruction.contains("/*FINAL_ANSWER*/"));

        let empty_response = crate::LlmResponseEnvelope {
            output_text: "   ".to_string(),
            ..crate::LlmResponseEnvelope::default()
        };
        let empty_effect = planner.process_response_effect(&ctx, &empty_response);
        assert!(!empty_effect.has_final_answer);
        assert!(empty_effect.output_text_override.is_none());

        let final_response = crate::LlmResponseEnvelope {
            output_text: "/*REASONING*/ think\n/*FINAL_ANSWER*/done".to_string(),
            ..crate::LlmResponseEnvelope::default()
        };
        let effect = planner.process_response_effect(&ctx, &final_response);
        assert_eq!(effect.output_text_override.as_deref(), Some("done"));
        assert!(effect.clear_output_text);
        assert!(effect.has_final_answer);
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
