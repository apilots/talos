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

const TOOL_USAGE_GUIDANCE: &str = "Tool usage guidance:\n- Prefer one tool call at a time unless the \
calls are clearly independent.\n- Tool choice stays model-driven: pick the tool that best matches \
the task rather than following a rigid checklist.\n- When the user provides a local absolute path, \
verify it with file tools before making environment claims about that path.\n- For a user-provided \
absolute directory path, list_dir is usually the best first look. For a user-provided absolute file \
path, read_file is usually the best first look.\n- Do not claim a path is outside the sandbox, \
outside the workspace, on the host filesystem, unmounted, or inaccessible until a file tool has \
actually failed on that path.\n- For codebase or repository investigation, prefer dedicated file \
tools over shell probes: use list_dir to map structure, grep_files to locate symbols or entrypoints, \
and read_file to inspect the specific files you narrowed down.\n- Avoid jumping into repeated \
read_file calls when grep_files can narrow the search first, and avoid repeating an equivalent \
read-only tool call unless you are verifying changed state or narrowing scope with materially \
different parameters.\n- Do not use exec_command as a substitute for file inspection or search when \
list_dir, read_file, or grep_files can answer the question directly. Reserve exec_command for cases \
where shell execution itself is the task.\n- Separate verified facts from inference. If the \
investigation is still shallow, present the result as a preliminary assessment rather than a full \
architecture conclusion.\n- When multiple tool calls are needed and their inputs do not depend on \
each other, call them in parallel in a single response. When a later call depends on the output of \
an earlier one, call them sequentially.\n- Before taking actions that are hard to reverse, confirm \
with the user: deleting files or directories, force-pushing or hard-resetting git history, or \
running commands that affect external services or shared state. Never use --no-verify, --force, or \
similar bypass flags unless the user explicitly requests it.";

const USER_FACING_PROGRESS_GUIDANCE: &str = "User-facing progress rules:\n- All normal text you \
output is shown to the user.\n- Before your first tool call, briefly state what you are about to \
do in one short sentence.\n- While working, give short progress updates when you find something \
load-bearing, when you change direction, or when enough time has passed without a visible update.\n- \
Keep these updates concise, concrete, and written for a human reader rather than as log lines.\n- \
After the tool-assisted exploration is complete, end with a clear final answer or summary.";

fn plan_react_instruction() -> String {
    format!(
        "When answering, prefer available tools over memorized knowledge.\n\nFollow this process:\n\
1. Start with a natural-language plan.\n2. Use tools to execute the plan and interleave tool use \
with reasoning updates.\n3. End with one final answer.\n\n{}\n\n{}\n\nUse this format:\n- planning under \
/*PLANNING*/\n- replanning under /*REPLANNING*/ when needed\n- reasoning under /*REASONING*/\n- \
tool actions under /*ACTION*/\n- final answer under /*FINAL_ANSWER*/.\n\nThe plan should be \
coherent, tool-grounded, and decomposed into numbered steps. If execution fails, revise the plan \
under /*REPLANNING*/. The final answer must be precise and explicitly state when the task cannot \
be completed with the available context.",
        TOOL_USAGE_GUIDANCE,
        USER_FACING_PROGRESS_GUIDANCE
    )
}

fn built_in_tool_guidance() -> String {
    format!(
        "Use available tools whenever they can verify facts, inspect the workspace, execute \
commands, or fetch concrete results. Do not answer from memory when a tool can check the answer.\n\n\
{}\n\n{}\n\nUse a direct answer only when no tool is needed.",
        TOOL_USAGE_GUIDANCE,
        USER_FACING_PROGRESS_GUIDANCE
    )
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
        assert!(instruction.contains("Prefer one tool call at a time"));
        assert!(instruction.contains("Tool choice stays model-driven"));
        assert!(instruction.contains("prefer dedicated file tools over shell probes"));
        assert!(instruction.contains("Separate verified facts from inference"));
        assert!(instruction.contains("Avoid jumping into repeated"));
        assert!(instruction.contains("Reserve exec_command for cases"));
        assert!(instruction.contains("Before your first tool call"));
        assert!(instruction.contains("give short progress updates"));
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
        assert!(instruction.contains("Prefer one tool call at a time"));
        assert!(instruction.contains("Tool choice stays model-driven"));
        assert!(instruction.contains("prefer dedicated file tools over shell probes"));
        assert!(instruction.contains("preliminary assessment"));
        assert!(instruction.contains("Avoid jumping into repeated"));
        assert!(instruction.contains("Reserve exec_command for cases"));
        assert!(instruction.contains("Before your first tool call"));
        assert!(instruction.contains("give short progress updates"));

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
