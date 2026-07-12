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

impl Default for BuiltInPlanner {
    fn default() -> Self {
        Self::new()
    }
}

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
        let planning_instruction =
            (!request.tool_definitions.is_empty()).then(built_in_tool_guidance);
        PlanningRequestEffect {
            planning_instruction,
            clear_request_thoughts: false,
        }
    }
}

#[cfg(test)]
#[path = "planner_tests.rs"]
mod tests;
