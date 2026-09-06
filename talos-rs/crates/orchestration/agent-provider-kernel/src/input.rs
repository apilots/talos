use talos_agent::AgentResult;
use talos_agent_api::{AgentMessage, AgentMessageBlockContent, AgentUserContent};
use talos_agent_kernel::UserInput;
use talos_error::TalosError;
use talos_skills::SkillSelector;
use talos_types::UserActionResponse;

pub(crate) struct KernelInputProjection {
    pub(crate) input: UserInput,
    pub(crate) skills: Vec<SkillSelector>,
    pub(crate) user_action: Option<(String, UserActionResponse)>,
}

pub(crate) fn project_input(message: &AgentMessage) -> AgentResult<KernelInputProjection> {
    let mut texts = Vec::new();
    let mut files = Vec::new();
    let mut skills = Vec::new();
    let mut user_action = None;

    for block in &message.blocks {
        let AgentMessageBlockContent::UserContent(content) = &block.content else {
            return Err(TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Agent Provider input contains non-User content",
            ));
        };
        match content {
            AgentUserContent::Text { text } => {
                let text = text.trim();
                if !text.is_empty() {
                    texts.push(text.to_string());
                }
            }
            AgentUserContent::SkillSelection { name, path } => {
                if !path.trim().is_empty() {
                    skills.push(SkillSelector::selection(name.trim(), path.trim()));
                }
            }
            AgentUserContent::File { file_id } => files.push(file_id.clone()),
            AgentUserContent::UserActionResponse {
                action_id,
                response,
            } => {
                if user_action
                    .replace((action_id.clone(), response.clone()))
                    .is_some()
                {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::InvalidRequest,
                        "Agent input contains multiple UserAction responses",
                    ));
                }
            }
            AgentUserContent::Audio { .. } | AgentUserContent::Video { .. } => {
                return Err(TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "Kernel Agent Provider does not support audio or video input",
                ));
            }
        }
    }

    Ok(KernelInputProjection {
        input: UserInput {
            text: texts.join("\n\n"),
            file_ids: files,
        },
        skills,
        user_action,
    })
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
