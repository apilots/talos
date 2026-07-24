//! History replay renderer boundary for render runtime.

use crate::render_runtime::common::style::StyledLine;
use crate::render_runtime::render::tool_projection;
use crate::render_runtime::render::user_input_display;
use crate::render_runtime::transcript::block::{FinalizedTranscriptBlock, TranscriptBlockKind};
use talos_sdk::{
    TalosTimelineContent, TalosTimelineItem, TalosTimelineItemKind, TalosTimelinePhase,
};

/// Converts a completed timeline history item into finalized transcript blocks.
pub(crate) fn timeline_item_to_blocks(item: &TalosTimelineItem) -> Vec<FinalizedTranscriptBlock> {
    match (&item.kind, &item.phase, &item.content) {
        (
            TalosTimelineItemKind::UserInput,
            TalosTimelinePhase::Input,
            TalosTimelineContent::Text { text },
        ) => text_block(TranscriptBlockKind::UserInput, text),
        (
            TalosTimelineItemKind::UserInput,
            TalosTimelinePhase::Input,
            TalosTimelineContent::UserInput { text, file_ids },
        ) => text_block(
            TranscriptBlockKind::UserInput,
            &user_input_display(text, file_ids),
        ),
        (
            TalosTimelineItemKind::Reasoning,
            TalosTimelinePhase::Thinking,
            TalosTimelineContent::Text { text },
        ) => text_block(TranscriptBlockKind::Thinking, text),
        (
            TalosTimelineItemKind::AssistantText,
            TalosTimelinePhase::FinalAnswer,
            TalosTimelineContent::Text { text },
        ) => text_block(TranscriptBlockKind::FinalAnswer, text),
        (
            TalosTimelineItemKind::AssistantText,
            TalosTimelinePhase::Unknown | TalosTimelinePhase::Commentary,
            TalosTimelineContent::Text { text },
        ) => text_block(TranscriptBlockKind::Commentary, text),
        (
            TalosTimelineItemKind::ToolCall,
            TalosTimelinePhase::Acting,
            TalosTimelineContent::ToolCall {
                call_id,
                name,
                arguments,
                progress,
                result,
            },
        ) => {
            let _ = progress;
            if let Some(result) = result {
                vec![tool_projection::tool_completed_block(
                    name,
                    call_id,
                    arguments,
                    result.is_error,
                    &result.content,
                )]
            } else {
                Vec::new()
            }
        }
        (
            TalosTimelineItemKind::SystemNotice,
            TalosTimelinePhase::System,
            TalosTimelineContent::SystemNotice { message, .. },
        ) => text_block(TranscriptBlockKind::SystemNotice, message),
        _ => Vec::new(),
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
    use super::timeline_item_to_blocks;
    use crate::render_runtime::transcript::block::TranscriptBlockKind;
    use talos_sdk::{
        TalosContentItem, TalosTimelineContent, TalosTimelineItem, TalosTimelineItemKind,
        TalosTimelineItemStatus, TalosTimelinePhase, TalosToolResult,
    };

    fn text_item(text: &str) -> TalosTimelineItem {
        TalosTimelineItem {
            item_id: "item-1".to_string(),
            parent_item_id: None,
            kind: TalosTimelineItemKind::AssistantText,
            phase: TalosTimelinePhase::FinalAnswer,
            status: talos_sdk::TalosTimelineItemStatus::Completed,
            title: None,
            started_at_ms: 1,
            completed_at_ms: Some(2),
            content: TalosTimelineContent::Text {
                text: text.to_string(),
            },
        }
    }

    #[test]
    fn skips_empty_text_history_items() {
        let item = text_item("   ");

        assert!(super::timeline_item_to_blocks(&item).is_empty());
    }

    #[test]
    fn converts_text_history_item_to_finalized_block() {
        let item = text_item("done");

        let blocks = timeline_item_to_blocks(&item);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, TranscriptBlockKind::FinalAnswer);
        assert_eq!(blocks[0].lines[0].plain_text(), "done");
    }

    #[test]
    fn converts_tool_history_item_to_copy_markdown() {
        let item = TalosTimelineItem {
            item_id: "tool-1".to_string(),
            parent_item_id: None,
            kind: TalosTimelineItemKind::ToolCall,
            phase: TalosTimelinePhase::Acting,
            status: TalosTimelineItemStatus::Completed,
            title: None,
            started_at_ms: 1,
            completed_at_ms: Some(2),
            content: TalosTimelineContent::ToolCall {
                call_id: "call-1".to_string(),
                name: "read".to_string(),
                arguments: r#"{"file_path":"/tmp/demo.py"}"#.to_string(),
                progress: Vec::new(),
                result: Some(TalosToolResult {
                    content: vec![TalosContentItem::Text {
                        text: "L1: print('hi')".to_string(),
                    }],
                    is_error: false,
                }),
            },
        };

        let blocks = timeline_item_to_blocks(&item);

        let markdown = blocks
            .last()
            .and_then(|block| block.copy_markdown.as_deref())
            .unwrap_or_default();
        assert!(markdown.contains("### Tool `Read`"));
        assert!(markdown.contains("\"file_path\": \"/tmp/demo.py\""));
        assert!(markdown.contains("Output:\n```text\nL1: print('hi')\n```"));
    }
}
