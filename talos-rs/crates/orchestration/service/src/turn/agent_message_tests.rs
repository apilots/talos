use talos_agent_api::{AgentMediaSource, AgentMessageBlockContent, AgentUserContent};
use talos_types::AgentUserInput;

use super::*;

#[test]
fn canonical_input_builds_one_completed_user_message() {
    let message = completed_agent_user_input(
        "turn-1",
        "message-1",
        AgentUserInput {
            content: vec![
                AgentUserContent::Text {
                    text: "hello".to_string(),
                },
                AgentUserContent::SkillSelection {
                    name: "review".to_string(),
                    path: "/skills/review".to_string(),
                },
                AgentUserContent::File {
                    file_id: "file-1".to_string(),
                },
                AgentUserContent::File {
                    file_id: "file-2".to_string(),
                },
                AgentUserContent::Audio {
                    source: AgentMediaSource::InlineBase64 {
                        data: "audio".to_string(),
                    },
                    mime_type: Some("audio/wav".to_string()),
                },
                AgentUserContent::Video {
                    source: AgentMediaSource::Url {
                        url: "https://example.test/video.mp4".to_string(),
                    },
                    mime_type: Some("video/mp4".to_string()),
                },
            ],
        },
        42,
    )
    .unwrap();

    assert_eq!(
        message
            .blocks
            .iter()
            .map(|block| block.content.clone())
            .collect::<Vec<_>>(),
        vec![
            AgentMessageBlockContent::UserContent(AgentUserContent::Text {
                text: "hello".to_string(),
            }),
            AgentMessageBlockContent::UserContent(AgentUserContent::SkillSelection {
                name: "review".to_string(),
                path: "/skills/review".to_string(),
            }),
            AgentMessageBlockContent::UserContent(AgentUserContent::File {
                file_id: "file-1".to_string(),
            }),
            AgentMessageBlockContent::UserContent(AgentUserContent::File {
                file_id: "file-2".to_string(),
            }),
            AgentMessageBlockContent::UserContent(AgentUserContent::Audio {
                source: AgentMediaSource::InlineBase64 {
                    data: "audio".to_string(),
                },
                mime_type: Some("audio/wav".to_string()),
            }),
            AgentMessageBlockContent::UserContent(AgentUserContent::Video {
                source: AgentMediaSource::Url {
                    url: "https://example.test/video.mp4".to_string(),
                },
                mime_type: Some("video/mp4".to_string()),
            }),
        ]
    );
    assert!(message
        .blocks
        .iter()
        .enumerate()
        .all(
            |(index, block)| block.block_id == format!("message-1_block_{index}")
                && block.started_at == 42
                && block.completed_at == Some(42)
        ));
}
