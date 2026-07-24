use std::path::PathBuf;

use talos_sdk::{ModelSelection, SkillSuggestion, StartTurnRequest, TalosContext, TalosInputItem};

pub struct TerminalTurnRequest {
    pub session_id: String,
    pub message: String,
    pub skills: Vec<SkillSuggestion>,
    pub file_ids: Vec<String>,
    pub context: Option<TalosContext>,
    pub model_selection: Option<ModelSelection>,
    pub current_directory: Option<PathBuf>,
    pub project_id: Option<String>,
    pub client_request_id: Option<String>,
}

impl TerminalTurnRequest {
    pub fn build(self) -> StartTurnRequest {
        let mut input = Vec::new();
        if !self.message.is_empty() {
            input.push(TalosInputItem::Text { text: self.message });
        }
        if !self.file_ids.is_empty() {
            input.push(TalosInputItem::Files {
                file_ids: self.file_ids,
            });
        }
        input.extend(
            self.skills
                .iter()
                .map(|skill| TalosInputItem::SkillSelection {
                    name: skill.name.clone(),
                    path: skill.path.to_string_lossy().to_string(),
                }),
        );

        StartTurnRequest {
            session_id: self.session_id,
            input,
            workspace_id: None,
            context: self.context,
            model_selection: self.model_selection.filter(|selection| {
                !selection.model_ref.provider.is_empty() && !selection.model_ref.id.is_empty()
            }),
            project_id: self.project_id,
            current_directory: self.current_directory,
            client_request_id: self.client_request_id,
        }
    }
}
