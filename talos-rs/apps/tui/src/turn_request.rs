use std::path::PathBuf;

use talos_sdk::{
    AgentUserContent, AgentUserInput, ModelSelection, SkillSuggestion, StartTurnRequest,
};

pub struct TerminalTurnRequest {
    pub session_id: String,
    pub message: String,
    pub skills: Vec<SkillSuggestion>,
    pub file_ids: Vec<String>,
    pub model_selection: Option<ModelSelection>,
    pub current_directory: Option<PathBuf>,
    pub project_id: Option<String>,
    pub client_request_id: Option<String>,
}

impl TerminalTurnRequest {
    pub fn build(self) -> StartTurnRequest {
        let input = build_terminal_input(self.message, &self.skills, self.file_ids);

        StartTurnRequest {
            node_id: None,
            session_id: self.session_id,
            input,
            model_selection: self.model_selection.filter(|selection| {
                !selection.model_ref.provider.is_empty() && !selection.model_ref.id.is_empty()
            }),
            user_permission_mode: None,
            project_id: self.project_id,
            current_directory: self.current_directory,
            client_request_id: self.client_request_id,
        }
    }
}

pub(crate) fn build_terminal_input(
    message: String,
    skills: &[SkillSuggestion],
    file_ids: Vec<String>,
) -> AgentUserInput {
    let mut content = Vec::new();
    if !message.is_empty() {
        content.push(AgentUserContent::Text { text: message });
    }
    content.extend(
        file_ids
            .into_iter()
            .map(|file_id| AgentUserContent::File { file_id }),
    );
    content.extend(skills.iter().map(|skill| AgentUserContent::SkillSelection {
        name: skill.name.clone(),
        path: skill.path.to_string_lossy().to_string(),
    }));
    AgentUserInput { content }
}
