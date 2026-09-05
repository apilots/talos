use talos_agent_api::{
    AgentMessageBlock, AgentMessageBlockStatus, AgentMessageRole, AgentMessageStatus,
};

use super::*;

#[test]
fn canonical_user_content_projects_once_into_kernel_input() {
    let message = AgentMessage {
        message_id: "message-1".to_string(),
        turn_id: "turn-1".to_string(),
        role: AgentMessageRole::User,
        status: AgentMessageStatus::Completed,
        blocks: vec![
            block(
                "1",
                AgentUserContent::Text {
                    text: " first ".to_string(),
                },
            ),
            block(
                "2",
                AgentUserContent::SkillSelection {
                    name: "review".to_string(),
                    path: "/skills/review".to_string(),
                },
            ),
            block(
                "3",
                AgentUserContent::File {
                    file_id: "file-1".to_string(),
                },
            ),
            block(
                "4",
                AgentUserContent::Text {
                    text: "second".to_string(),
                },
            ),
        ],
        created_at: 1,
        completed_at: Some(1),
    };

    let projected = project_input(&message).unwrap();

    assert_eq!(
        projected.input,
        UserInput {
            text: "first\n\nsecond".to_string(),
            file_ids: vec!["file-1".to_string()],
        }
    );
    assert_eq!(projected.skills.len(), 1);
}

fn block(block_id: &str, content: AgentUserContent) -> AgentMessageBlock {
    AgentMessageBlock {
        block_id: block_id.to_string(),
        status: AgentMessageBlockStatus::Completed,
        content: AgentMessageBlockContent::UserContent(content),
        started_at: 1,
        completed_at: Some(1),
    }
}
