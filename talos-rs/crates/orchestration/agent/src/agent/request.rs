use crate::service::AgentRunParams;
use talos_agent_kernel::TurnPhase;
use talos_skills::SkillSelector;
use talos_types::talos::{RunItem, TalosInputItem, TalosReasoning};
use talos_types::TalosSession;

#[derive(Debug, Clone)]
pub(crate) struct AgentKernelParams {
    pub(crate) session: TalosSession,
    pub(crate) input: Vec<TalosInputItem>,
    pub(crate) phase: TurnPhase,
    pub(crate) planning_instruction: Option<String>,
    pub(crate) transfer_to: Option<String>,
}

impl AgentKernelParams {
    pub(crate) fn extract_text_input(input: &[TalosInputItem]) -> String {
        input
            .iter()
            .filter_map(|item| match item {
                TalosInputItem::Text { text } => {
                    let normalized = text.trim();
                    if normalized.is_empty() {
                        None
                    } else {
                        Some(normalized.to_string())
                    }
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub(crate) fn extract_skill_selections(input: &[TalosInputItem]) -> Vec<SkillSelector> {
        input
            .iter()
            .filter_map(|item| match item {
                TalosInputItem::SkillSelection { name, path } if !path.trim().is_empty() => {
                    Some(SkillSelector::selection(name.trim(), path.trim()))
                }
                _ => None,
            })
            .collect()
    }

    pub(crate) fn collect_reasoning(output_items: &[RunItem]) -> Vec<TalosReasoning> {
        output_items
            .iter()
            .filter_map(|item| match item {
                RunItem::Reasoning { text } => Some(TalosReasoning { text: text.clone() }),
                _ => None,
            })
            .collect()
    }
}

pub(crate) fn agent_run_params_to_kernel_params(request: AgentRunParams) -> AgentKernelParams {
    AgentKernelParams {
        session: request.session,
        input: request.input,
        phase: TurnPhase::Execution,
        planning_instruction: None,
        transfer_to: None,
    }
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
