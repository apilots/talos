use crate::service::{AgentRunConfig, AgentRunContext, AgentRunParams};
use talos_agent_kernel::{TurnPhase, UserInput};
use talos_error::{TalosError, TalosResult};
use talos_skills::SkillSelector;
use talos_types::talos::{RunItem, TalosInputItem, TalosReasoning};

/// Kernel-specific request shape derived from shared AgentRunParams.
///
/// This adapter object is crate-private. It can carry Kernel execution details,
/// but it must not replace the shared Harness/Runtime AgentRunParams boundary.
#[derive(Debug, Clone)]
pub(crate) struct AgentKernelParams {
    pub(crate) context: AgentRunContext,
    pub(crate) config: AgentRunConfig,
    pub(crate) input: Vec<TalosInputItem>,
    pub(crate) phase: TurnPhase,
    pub(crate) planning_instruction: Option<String>,
    pub(crate) transfer_to: Option<String>,
}

impl AgentKernelParams {
    /// Normalize text and ordered file references into one Agent input object.
    pub(crate) fn extract_user_input(input: &[TalosInputItem]) -> TalosResult<UserInput> {
        let files = input
            .iter()
            .filter_map(|item| match item {
                TalosInputItem::Files { file_ids } => Some(file_ids),
                _ => None,
            })
            .collect::<Vec<_>>();
        if files.len() > 1 {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "input must include at most one files item".to_string(),
            ));
        }
        Ok(UserInput {
            text: Self::extract_text_input(input),
            file_ids: files.first().map(|ids| (*ids).clone()).unwrap_or_default(),
        })
    }

    /// Extract normalized user text from shared Agent input items.
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

    /// Extract skill selections from shared Agent input items.
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

    /// Convert shared run transcript items into Talos reasoning summaries.
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

/// Convert shared Agent run params into Kernel-private execution params.
pub(crate) fn agent_run_params_to_kernel_params(request: AgentRunParams) -> AgentKernelParams {
    AgentKernelParams {
        context: request.context,
        config: request.config,
        input: request.input,
        phase: TurnPhase::Execution,
        planning_instruction: None,
        transfer_to: None,
    }
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
