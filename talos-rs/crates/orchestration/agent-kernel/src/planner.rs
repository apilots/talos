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

fn plan_react_instruction() -> String {
    "Use tools when needed and keep the response grounded in observed results.\n\nFollow this \
process:\n1. Start with a concise natural-language plan.\n2. Interleave tool use with short \
reasoning updates.\n3. Revise the plan only when evidence changes.\n4. End with one final answer.\n\n\
Use this format:\n- planning under /*PLANNING*/\n- replanning under /*REPLANNING*/ when needed\n- \
reasoning under /*REASONING*/\n- tool actions under /*ACTION*/\n- final answer under \
/*FINAL_ANSWER*/.\n\nThe final answer must be precise and explicitly state when the task cannot be \
completed with the available context."
        .to_string()
}

fn built_in_tool_guidance() -> String {
    "Use available tools whenever they can verify facts, inspect the workspace, execute commands, \
or fetch concrete results. Do not answer from memory when a tool can check the answer. If the user \
explicitly names a tool and it can satisfy the next verification step, honor that tool first. For \
codebase analysis, prefer grep_files before read_file, and use list_dir first only when directory \
shape or path type is the missing fact. Use grep_files instead of shell grep or rg, read_file \
instead of cat or head, and list_dir instead of ls or find. Do not replace grep_files, read_file, \
or list_dir with exec_command when dedicated tools can answer directly. Use a direct answer only \
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
        let native_thinking = if request
            .selected_model_info
            .as_ref()
            .map(|model| model.reasoning)
            .unwrap_or(false)
        {
            Some(NativeThinkingConfig {
                effort: NativeThinkingEffort::Medium,
                budget_tokens: Some(2048),
            })
        } else {
            None
        };
        let planning_instruction =
            (!request.tool_definitions.is_empty()).then(built_in_tool_guidance);
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
            planning_instruction: Some(plan_react_instruction()),
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
            project_root: None,
            input: input.to_string(),
            selected_model_info: talos_types::ModelInfo {
                id: "m1".to_string(),
                name: "m1".to_string(),
                provider: "p1".to_string(),
                context_window: 0,
                max_tokens: 0,
                reasoning: false,
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
        supported_request.selected_model_info = Some(talos_types::ModelInfo {
            id: "m1".to_string(),
            name: "m1".to_string(),
            provider: "p1".to_string(),
            context_window: 0,
            max_tokens: 0,
            reasoning: true,
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        });
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
        assert!(instruction.contains("Do not answer from memory"));
        assert!(instruction.contains("honor that tool first"));
        assert!(instruction.contains("prefer grep_files before read_file"));
        assert!(instruction.contains("use list_dir first only when directory shape"));
        assert!(instruction.contains("instead of shell grep or rg"));
        assert!(instruction.contains("instead of cat or head"));
        assert!(instruction.contains("instead of ls or find"));
        assert!(instruction.contains("Use a direct answer only when no tool is needed"));
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
        assert!(instruction.contains("Interleave tool use with short reasoning updates"));
        assert!(instruction.contains("Revise the plan only when evidence changes"));
        assert!(instruction.contains("completed with the available context"));

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
