use super::AgentHarness;
use crate::service::{AgentRequest, OrchestratorRequest};
use talos_agent_kernel::TurnPhase;
use talos_skills::SkillSelector;
use talos_types::talos::{TalosInputItem, TalosOutputItem, TalosReasoning};
use talos_types::{ModelInfo, TalosSession};

impl AgentHarness {
    pub(crate) fn resolve_selected_model_info(&self, session: &TalosSession) -> ModelInfo {
        let selected_model = if session.config.llm.model.trim().is_empty() {
            self.model.clone()
        } else {
            session.config.llm.model.clone()
        };
        let provider_name = session.config.llm.provider.clone();

        if let Some(provider_config) = session.config.current_provider_config() {
            if let Some(model) = provider_config
                .models
                .iter()
                .find(|m| m.id == selected_model)
            {
                return ModelInfo {
                    id: model.id.clone(),
                    name: model.display_name().to_string(),
                    provider: provider_name,
                    context_window: model.context_window,
                    reasoning: model.reasoning,
                    reasoning_effort: model.reasoning_effort,
                    speed: model.speed,
                    input: model.input.clone(),
                    cost: model.cost.clone(),
                    compat: model.compat.clone(),
                };
            }
        }

        ModelInfo {
            id: selected_model.clone(),
            name: selected_model,
            provider: provider_name,
            context_window: 0,
            reasoning: false,
            reasoning_effort: Default::default(),
            speed: Default::default(),
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        }
    }

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

    pub(crate) fn collect_reasoning(output_items: &[TalosOutputItem]) -> Vec<TalosReasoning> {
        output_items
            .iter()
            .filter_map(|item| match item {
                TalosOutputItem::Reasoning { text } => Some(TalosReasoning { text: text.clone() }),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn to_agent_request(&self, request: OrchestratorRequest) -> AgentRequest {
        AgentRequest {
            session: request.session,
            input: request.input,
            phase: TurnPhase::Execution,
            planning_instruction: None,
            transfer_to: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn extracts_structured_skill_selections_with_path() {
        let selections = AgentHarness::extract_skill_selections(&[
            TalosInputItem::Text {
                text: "Use $review".to_string(),
            },
            TalosInputItem::SkillSelection {
                name: " review ".to_string(),
                path: " /tmp/review/SKILL.md ".to_string(),
            },
            TalosInputItem::SkillSelection {
                name: "ignored".to_string(),
                path: " ".to_string(),
            },
        ]);

        assert_eq!(
            selections,
            vec![SkillSelector::selection(
                "review",
                PathBuf::from("/tmp/review/SKILL.md")
            )]
        );
    }
}
