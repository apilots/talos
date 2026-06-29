use std::path::PathBuf;

use talos_sdk::{SkillSuggestion, StartTurnRequest, TalosContext, TalosInputItem};

pub struct TerminalTurnRequest {
    pub session_id: String,
    pub message: String,
    pub skills: Vec<SkillSuggestion>,
    pub context: Option<TalosContext>,
    pub model: Option<String>,
    pub current_directory: Option<PathBuf>,
    pub client_request_id: Option<String>,
}

impl TerminalTurnRequest {
    pub fn build(self) -> StartTurnRequest {
        let mut input = vec![TalosInputItem::Text { text: self.message }];
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
            model: self.model.filter(|model| !model.is_empty()),
            project_id: None,
            current_directory: self.current_directory,
            client_request_id: self.client_request_id,
        }
    }
}
