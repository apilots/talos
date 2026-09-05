//! Canonical Agent history replay renderer boundary.

use std::collections::HashMap;

use crate::render_runtime::common::style::StyledLine;
use crate::render_runtime::render::tool_projection;
use crate::render_runtime::transcript::block::{FinalizedTranscriptBlock, TranscriptBlockKind};
use talos_sdk::{
    AgentAssistantTextPhase, AgentMessage, AgentMessageBlockContent, AgentToolCallArguments,
    AgentUserContent,
};

/// Converts one completed canonical Agent message into transcript blocks.
pub(crate) fn agent_message_to_blocks(message: &AgentMessage) -> Vec<FinalizedTranscriptBlock> {
    let tool_calls = message
        .blocks
        .iter()
        .filter_map(|block| match &block.content {
            AgentMessageBlockContent::ToolCall(tool) => Some((tool.call_id.as_str(), tool)),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let mut blocks = Vec::new();
    for block in &message.blocks {
        let mut rendered = match &block.content {
            AgentMessageBlockContent::UserContent(content) => {
                text_block(TranscriptBlockKind::UserInput, &user_content_text(content))
            }
            AgentMessageBlockContent::Reasoning(reasoning) => {
                text_block(TranscriptBlockKind::Thinking, &reasoning.text)
            }
            AgentMessageBlockContent::AssistantText(assistant) => text_block(
                match assistant.phase {
                    AgentAssistantTextPhase::Pending | AgentAssistantTextPhase::Commentary => {
                        TranscriptBlockKind::Commentary
                    }
                    AgentAssistantTextPhase::FinalAnswer => TranscriptBlockKind::FinalAnswer,
                },
                &assistant.text,
            ),
            AgentMessageBlockContent::ToolResult(result) => tool_calls
                .get(result.call_id.as_str())
                .map(|tool| {
                    vec![tool_projection::tool_completed_block(
                        &tool.name,
                        &tool.call_id,
                        &tool_arguments_text(&tool.arguments),
                        result.is_error,
                        &result.content,
                    )]
                })
                .unwrap_or_default(),
            AgentMessageBlockContent::ExecutionSummary(_) => Vec::new(),
            AgentMessageBlockContent::ContextCompaction(compaction) => text_block(
                TranscriptBlockKind::SystemNotice,
                compaction
                    .message
                    .as_deref()
                    .unwrap_or("Context compaction completed"),
            ),
            AgentMessageBlockContent::SystemNotice(notice) => {
                text_block(TranscriptBlockKind::SystemNotice, &notice.message)
            }
            AgentMessageBlockContent::ToolCall(_) => Vec::new(),
        };
        blocks.append(&mut rendered);
    }
    blocks
}

fn user_content_text(content: &AgentUserContent) -> String {
    match content {
        AgentUserContent::Text { text } => text.clone(),
        AgentUserContent::SkillSelection { name, .. } => format!("[skill:{name}]"),
        AgentUserContent::File { file_id } => format!("[file:{file_id}]"),
        AgentUserContent::Audio { .. } => "[audio]".to_string(),
        AgentUserContent::Video { .. } => "[video]".to_string(),
        AgentUserContent::UserActionResponse { action_id, .. } => {
            format!("[user action response:{action_id}]")
        }
    }
}

fn tool_arguments_text(arguments: &AgentToolCallArguments) -> String {
    match arguments {
        AgentToolCallArguments::Streaming { json_fragment } => json_fragment.clone(),
        AgentToolCallArguments::Complete { value } => serde_json::to_string(value)
            .unwrap_or_else(|error| format!("<invalid tool arguments: {error}>")),
    }
}

fn text_block(kind: TranscriptBlockKind, text: &str) -> Vec<FinalizedTranscriptBlock> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    vec![FinalizedTranscriptBlock::new(
        kind,
        vec![StyledLine::plain(text.to_string())],
    )]
}

#[cfg(test)]
mod tests {
    use super::agent_message_to_blocks;
    use crate::render_runtime::transcript::block::TranscriptBlockKind;
    use talos_sdk::{
        AgentAssistantText, AgentAssistantTextPhase, AgentMessage, AgentMessageBlock,
        AgentMessageBlockContent, AgentMessageBlockStatus, AgentMessageRole, AgentMessageStatus,
        AgentToolCall, AgentToolCallArguments, AgentToolResult, TalosContentItem,
    };

    fn message(blocks: Vec<AgentMessageBlock>) -> AgentMessage {
        AgentMessage {
            message_id: "message-1".to_string(),
            turn_id: "turn-1".to_string(),
            role: AgentMessageRole::Agent,
            status: AgentMessageStatus::Completed,
            blocks,
            created_at: 1,
            completed_at: Some(2),
        }
    }

    fn text_block(text: &str) -> AgentMessageBlock {
        AgentMessageBlock {
            block_id: "block-1".to_string(),
            status: AgentMessageBlockStatus::Completed,
            content: AgentMessageBlockContent::AssistantText(AgentAssistantText {
                phase: AgentAssistantTextPhase::FinalAnswer,
                text: text.to_string(),
            }),
            started_at: 1,
            completed_at: Some(2),
        }
    }

    #[test]
    fn skips_empty_text_history_items() {
        let message = message(vec![text_block("   ")]);

        assert!(agent_message_to_blocks(&message).is_empty());
    }

    #[test]
    fn converts_text_history_item_to_finalized_block() {
        let message = message(vec![text_block("done")]);

        let blocks = agent_message_to_blocks(&message);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, TranscriptBlockKind::FinalAnswer);
        assert_eq!(blocks[0].lines[0].plain_text(), "done");
    }

    #[test]
    fn converts_tool_history_item_to_copy_markdown() {
        let message = message(vec![
            AgentMessageBlock {
                block_id: "tool-1".to_string(),
                status: AgentMessageBlockStatus::Completed,
                content: AgentMessageBlockContent::ToolCall(AgentToolCall {
                    call_id: "call-1".to_string(),
                    name: "read".to_string(),
                    arguments: AgentToolCallArguments::Streaming {
                        json_fragment: r#"{"file_path":"/tmp/demo.py"}"#.to_string(),
                    },
                    progress: Vec::new(),
                }),
                started_at: 1,
                completed_at: Some(2),
            },
            AgentMessageBlock {
                block_id: "result-1".to_string(),
                status: AgentMessageBlockStatus::Completed,
                content: AgentMessageBlockContent::ToolResult(AgentToolResult {
                    call_id: "call-1".to_string(),
                    tool_name: "read".to_string(),
                    content: vec![TalosContentItem::Text {
                        text: "L1: print('hi')".to_string(),
                    }],
                    is_error: false,
                }),
                started_at: 2,
                completed_at: Some(2),
            },
        ]);

        let blocks = agent_message_to_blocks(&message);

        let markdown = blocks
            .last()
            .and_then(|block| block.copy_markdown.as_deref())
            .unwrap_or_default();
        assert!(markdown.contains("### Tool `Read`"));
        assert!(markdown.contains("\"file_path\": \"/tmp/demo.py\""));
        assert!(markdown.contains("Output:\n```text\nL1: print('hi')\n```"));
    }
}
