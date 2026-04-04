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
Tool usage rules:\n- Prefer one tool call at a time unless the calls are clearly independent.\n- \
When the user provides a local absolute path, treat it as a path you should verify with file \
tools before making environment claims.\n- For a user-provided absolute directory path, start \
with list_dir. For a user-provided absolute file path, start with read_file.\n- Do not claim a \
path is outside the sandbox, outside the workspace, on the host filesystem, unmounted, or \
inaccessible until a file tool has actually failed on that path.\n- For local path analysis, do \
not start with memory_search or web_search when local file tools can inspect the path directly.\n- \
For codebase investigation, narrow scope before reading files: start with directory listing or \
globbing, then use grep/search, then read specific files.\n- Do not jump into repeated read_file \
calls when grep/search can locate symbols or call sites first.\n- For repository or architecture \
analysis, do not stop after a directory listing. Inspect the top-level structure, read at least \
one manifest or README file, and run at least one grep/search pass for entrypoints, modules, or \
key symbols before writing the summary.\n- Separate verified facts from inference. If the \
investigation is still shallow, say it is a preliminary assessment rather than a full architecture \
analysis.\n- For repository or architecture analysis, the first investigation pass should normally \
be list_dir or grep_files, not read_file, unless the user already named an exact file to inspect.\n- \
For repository or architecture analysis, a strong default sequence is: 1) list_dir at the repo \
root, 2) read README or a manifest such as Cargo.toml/package.json/pyproject.toml/go.mod, 3) \
grep_files for entrypoints and modules using patterns like main, app, server, router, cli, or \
register, 4) then read the specific files you found.\n- A summary based on only one directory \
listing or one file read is usually insufficient for architecture analysis.\n\n\
Use this format:\n- planning under /*PLANNING*/\n- replanning under /*REPLANNING*/ when needed\n\
- reasoning under /*REASONING*/\n- tool actions under /*ACTION*/\n- final answer under \
/*FINAL_ANSWER*/.\n\nThe plan should be coherent, tool-grounded, and decomposed into numbered \
steps. If execution fails, revise the plan under /*REPLANNING*/. The final answer must be precise \
and explicitly state when the task cannot be completed with the available context.";

const BUILT_IN_TOOL_GUIDANCE: &str = "Use available tools whenever they can verify facts, inspect \
the workspace, execute commands, or fetch concrete results. Do not answer from memory when a tool \
can check the answer. Prefer one tool call at a time unless parallel work is clearly necessary. \
When the user provides a local absolute path, verify it with local file tools before making any \
claim about sandbox limits, workspace boundaries, mount state, or filesystem access. For a \
directory path, start with list_dir. For a file path, start with read_file. Do not default to \
memory_search or web_search for local path analysis when file tools can inspect the target \
directly. \
For repository analysis, narrow scope before opening files: start with list_dir or glob patterns, \
then grep/search for symbols or paths, then read the specific files you need. For repository or \
architecture analysis, do not stop after a directory listing: inspect the top-level structure, \
read at least one manifest or README file, and run at least one grep/search pass for entrypoints, \
modules, or key symbols before summarizing. Separate verified facts from inference, and label the \
result as preliminary when the investigation is still shallow. For repository or architecture \
analysis, the first investigation pass should normally be list_dir or grep_files, not read_file, \
unless the user already named an exact file to inspect. A strong default sequence is: 1) list_dir \
at the repo root, 2) read README or a manifest such as Cargo.toml/package.json/pyproject.toml/go.mod, \
3) grep_files for entrypoints and modules using patterns like main, app, server, router, cli, or \
register, 4) then read the specific files you found. A summary based on only one directory \
listing or one file read is usually insufficient for architecture analysis. Use a direct answer \
only when no tool is needed.";

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
        assert!(instruction.contains("Prefer one tool call at a time"));
        assert!(instruction.contains("then grep/search"));
        assert!(instruction.contains("read at least one manifest or README file"));
        assert!(instruction.contains("Separate verified facts from inference"));
        assert!(instruction
            .contains("first investigation pass should normally be list_dir or grep_files"));
        assert!(instruction.contains("strong default sequence"));
        assert!(instruction.contains("Cargo.toml/package.json/pyproject.toml/go.mod"));
        assert!(instruction.contains("usually insufficient for architecture analysis"));
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
        assert!(instruction.contains("then use grep/search"));
        assert!(instruction.contains("read at least one manifest or README file"));
        assert!(instruction.contains("preliminary assessment"));
        assert!(instruction
            .contains("first investigation pass should normally be list_dir or grep_files"));
        assert!(instruction.contains("strong default sequence"));
        assert!(instruction.contains("Cargo.toml/package.json/pyproject.toml/go.mod"));
        assert!(instruction.contains("usually insufficient for architecture analysis"));

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
