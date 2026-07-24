//! Timeline event adapter for the render runtime live reducer.

use talos_sdk::{
    TalosTimelineContent, TalosTimelineDelta, TalosTimelineItem, TalosTimelineItemKind,
    TalosTimelinePhase,
};

use crate::render_runtime::common::style::StyledLine;
use crate::render_runtime::render::live::{LiveReducer, LiveTextKind};
use crate::render_runtime::render::user_input_display;
use crate::render_runtime::transcript::block::{FinalizedTranscriptBlock, TranscriptBlockKind};

/// Applies an item-started event to render runtime live state.
pub(crate) fn apply_item_started(
    live: &mut LiveReducer,
    item: &TalosTimelineItem,
) -> Vec<FinalizedTranscriptBlock> {
    match (&item.kind, &item.phase, &item.content) {
        (
            TalosTimelineItemKind::UserInput,
            TalosTimelinePhase::Input,
            TalosTimelineContent::Text { text },
        ) => text_finalized_block(TranscriptBlockKind::UserInput, text),
        (
            TalosTimelineItemKind::UserInput,
            TalosTimelinePhase::Input,
            TalosTimelineContent::UserInput { text, file_ids },
        ) => text_finalized_block(
            TranscriptBlockKind::UserInput,
            &user_input_display(text, file_ids),
        ),
        (_, _, TalosTimelineContent::Text { text }) => {
            if let Some((kind, phase_resolved)) = live_text_state(item) {
                if phase_resolved {
                    live.start_text(&item.item_id, kind, text);
                } else {
                    live.start_unresolved_text(&item.item_id, kind, text);
                }
            }
            Vec::new()
        }
        (
            TalosTimelineItemKind::ToolCall,
            TalosTimelinePhase::Acting,
            TalosTimelineContent::ToolCall {
                call_id,
                name,
                arguments,
                ..
            },
        ) => {
            live.start_tool_with_arguments(&item.item_id, call_id, name, arguments);
            Vec::new()
        }
        (
            TalosTimelineItemKind::SystemNotice,
            TalosTimelinePhase::System,
            TalosTimelineContent::SystemNotice { message, .. },
        ) => text_finalized_block(TranscriptBlockKind::SystemNotice, message),
        _ => Vec::new(),
    }
}

/// Applies a timeline delta to render runtime live state.
pub(crate) fn apply_item_delta(
    live: &mut LiveReducer,
    item: &TalosTimelineItem,
    delta: TalosTimelineDelta,
) -> Vec<FinalizedTranscriptBlock> {
    match delta {
        TalosTimelineDelta::Text { text } => {
            if let Some((kind, phase_resolved)) = live_text_state(item) {
                if phase_resolved {
                    live.start_text(&item.item_id, kind, "");
                } else {
                    live.start_unresolved_text(&item.item_id, kind, "");
                }
                live.append_text(&item.item_id, &text);
            }
            Vec::new()
        }
        TalosTimelineDelta::ToolProgress {
            progress_kind,
            text_delta,
        } => {
            live.append_tool_progress_delta(&item.item_id, progress_kind, &text_delta);
            Vec::new()
        }
        TalosTimelineDelta::ToolResult { content, is_error } => live
            .complete_tool(&item.item_id, is_error, content)
            .into_iter()
            .collect(),
        TalosTimelineDelta::ToolDynamicRequest {
            call_id, tool_name, ..
        } => {
            live.start_tool(&item.item_id, call_id, tool_name);
            Vec::new()
        }
        TalosTimelineDelta::ToolArguments { arguments_delta } => {
            live.append_tool_arguments(&item.item_id, &arguments_delta);
            Vec::new()
        }
        TalosTimelineDelta::ContextCompaction { .. } | TalosTimelineDelta::Status { .. } => {
            Vec::new()
        }
    }
}

/// Applies an item-completed event to render runtime live state.
pub(crate) fn apply_item_completed(
    live: &mut LiveReducer,
    item: &TalosTimelineItem,
    final_content: Option<&TalosTimelineContent>,
) -> Vec<FinalizedTranscriptBlock> {
    if let Some(content) = final_content {
        match content {
            TalosTimelineContent::Text { text } => {
                if let Some((kind, phase_resolved)) = live_text_state(item) {
                    if phase_resolved {
                        live.start_text(&item.item_id, kind, "");
                    } else {
                        live.start_unresolved_text(&item.item_id, kind, "");
                    }
                    live.replace_text(&item.item_id, text.clone());
                }
            }
            TalosTimelineContent::ToolCall { result, .. } => {
                if let Some(result) = result {
                    return live
                        .complete_tool(&item.item_id, result.is_error, result.content.clone())
                        .into_iter()
                        .collect();
                }
            }
            TalosTimelineContent::SystemNotice { message, .. } => {
                return text_finalized_block(TranscriptBlockKind::SystemNotice, message);
            }
            TalosTimelineContent::ExecutionSummary { .. }
            | TalosTimelineContent::UserInput { .. }
            | TalosTimelineContent::ContextCompaction { .. }
            | TalosTimelineContent::Empty => {}
        }
    }

    if live_text_state(item).is_some() {
        return live.complete_text(&item.item_id).into_iter().collect();
    }
    Vec::new()
}

/// Applies an assistant text committed event to render runtime live state.
pub(crate) fn apply_assistant_text_committed(
    live: &mut LiveReducer,
    item_id: &str,
    phase: TalosTimelinePhase,
) -> Option<FinalizedTranscriptBlock> {
    match phase {
        TalosTimelinePhase::FinalAnswer => live.commit_text(item_id, LiveTextKind::FinalAnswer),
        TalosTimelinePhase::Commentary => live.commit_text(item_id, LiveTextKind::Commentary),
        TalosTimelinePhase::Unknown
        | TalosTimelinePhase::Input
        | TalosTimelinePhase::Thinking
        | TalosTimelinePhase::Acting
        | TalosTimelinePhase::System => None,
    }
}

fn live_text_state(item: &TalosTimelineItem) -> Option<(LiveTextKind, bool)> {
    match (&item.kind, item.phase) {
        (TalosTimelineItemKind::Reasoning, TalosTimelinePhase::Thinking) => {
            Some((LiveTextKind::Thinking, true))
        }
        (TalosTimelineItemKind::AssistantText, TalosTimelinePhase::FinalAnswer) => {
            Some((LiveTextKind::FinalAnswer, true))
        }
        (TalosTimelineItemKind::AssistantText, TalosTimelinePhase::Commentary) => {
            Some((LiveTextKind::Commentary, true))
        }
        (TalosTimelineItemKind::AssistantText, TalosTimelinePhase::Unknown) => {
            Some((LiveTextKind::Commentary, false))
        }
        _ => None,
    }
}

fn text_finalized_block(kind: TranscriptBlockKind, text: &str) -> Vec<FinalizedTranscriptBlock> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    vec![FinalizedTranscriptBlock::new(
        kind,
        StyledLine::plain_lines(text),
    )]
}

#[cfg(test)]
mod tests {
    use super::{
        apply_assistant_text_committed, apply_item_completed, apply_item_delta, apply_item_started,
    };
    use crate::render_runtime::render::live::LiveReducer;
    use crate::render_runtime::transcript::block::TranscriptBlockKind;
    use talos_sdk::{
        TalosContentItem, TalosTimelineContent, TalosTimelineDelta, TalosTimelineItem,
        TalosTimelineItemKind, TalosTimelineItemStatus, TalosTimelinePhase,
    };

    fn text_item(
        kind: TalosTimelineItemKind,
        phase: TalosTimelinePhase,
        text: &str,
    ) -> TalosTimelineItem {
        TalosTimelineItem {
            item_id: "item-1".to_string(),
            parent_item_id: None,
            kind,
            phase,
            status: TalosTimelineItemStatus::Running,
            title: None,
            started_at_ms: 1,
            completed_at_ms: None,
            content: TalosTimelineContent::Text {
                text: text.to_string(),
            },
        }
    }

    fn tool_item() -> TalosTimelineItem {
        TalosTimelineItem {
            item_id: "tool-1".to_string(),
            parent_item_id: None,
            kind: TalosTimelineItemKind::ToolCall,
            phase: TalosTimelinePhase::Acting,
            status: TalosTimelineItemStatus::Running,
            title: None,
            started_at_ms: 1,
            completed_at_ms: None,
            content: TalosTimelineContent::ToolCall {
                call_id: "call-1".to_string(),
                name: "Read".to_string(),
                arguments: r#"{"file_path":"/tmp/demo.py"}"#.to_string(),
                progress: Vec::new(),
                result: None,
            },
        }
    }

    #[test]
    fn text_deltas_stay_mutable_until_item_completed() {
        let mut live = LiveReducer::default();
        let item = text_item(
            TalosTimelineItemKind::AssistantText,
            TalosTimelinePhase::Unknown,
            "",
        );

        assert!(apply_item_started(&mut live, &item).is_empty());
        assert!(apply_item_delta(
            &mut live,
            &item,
            TalosTimelineDelta::Text {
                text: "hello".to_string(),
            },
        )
        .is_empty());
        assert!(apply_assistant_text_committed(
            &mut live,
            &item.item_id,
            TalosTimelinePhase::FinalAnswer
        )
        .is_none());

        let finalized = apply_item_completed(&mut live, &item, None);

        assert_eq!(finalized.len(), 1);
        assert_eq!(finalized[0].kind, TranscriptBlockKind::FinalAnswer);
        assert_eq!(finalized[0].lines[0].plain_text(), "hello");
    }

    #[test]
    fn final_content_replaces_mutable_text_before_finalizing() {
        let mut live = LiveReducer::default();
        let item = text_item(
            TalosTimelineItemKind::AssistantText,
            TalosTimelinePhase::FinalAnswer,
            "partial",
        );
        apply_item_started(&mut live, &item);

        let finalized = apply_item_completed(
            &mut live,
            &item,
            Some(&TalosTimelineContent::Text {
                text: "complete".to_string(),
            }),
        );

        assert_eq!(finalized[0].lines[0].plain_text(), "complete");
    }

    #[test]
    fn completed_unknown_assistant_text_waits_for_phase_commit() {
        let mut live = LiveReducer::default();
        let item = text_item(
            TalosTimelineItemKind::AssistantText,
            TalosTimelinePhase::Unknown,
            "partial",
        );
        apply_item_started(&mut live, &item);

        let finalized = apply_item_completed(
            &mut live,
            &item,
            Some(&TalosTimelineContent::Text {
                text: "final\nanswer".to_string(),
            }),
        );

        assert!(finalized.is_empty());
        let finalized = apply_assistant_text_committed(
            &mut live,
            &item.item_id,
            TalosTimelinePhase::FinalAnswer,
        )
        .unwrap();
        assert_eq!(finalized.kind, TranscriptBlockKind::FinalAnswer);
        assert_eq!(finalized.lines[0].plain_text(), "final");
        assert_eq!(finalized.lines[1].plain_text(), "answer");
    }

    #[test]
    fn tool_progress_and_result_are_append_only_blocks() {
        let mut live = LiveReducer::default();
        let item = tool_item();

        let header = apply_item_started(&mut live, &item);
        let progress = apply_item_delta(
            &mut live,
            &item,
            TalosTimelineDelta::ToolProgress {
                progress_kind: talos_sdk::TalosToolProgressKind::Stdout,
                text_delta: "Read 1 line".to_string(),
            },
        );
        let result = apply_item_delta(
            &mut live,
            &item,
            TalosTimelineDelta::ToolResult {
                content: vec![TalosContentItem::Text {
                    text: "ok".to_string(),
                }],
                is_error: false,
            },
        );

        assert!(header.is_empty());
        assert!(progress.is_empty());
        assert_eq!(
            result[0].lines[0].plain_text(),
            "● Read(/tmp/demo.py) completed"
        );
        assert_eq!(result[0].lines[1].plain_text(), "  ⎿ ok (ctrl+o to expand)");
        let markdown = result[0].copy_markdown.as_deref().unwrap_or_default();
        assert!(markdown.contains("### Tool `Read`"));
        assert!(markdown.contains("\"file_path\": \"/tmp/demo.py\""));
        assert!(markdown.contains("Output:\n```text\nok\n```"));
    }

    #[test]
    fn tool_argument_delta_updates_completed_title() {
        let mut live = LiveReducer::default();
        let mut item = tool_item();
        if let TalosTimelineContent::ToolCall { arguments, .. } = &mut item.content {
            arguments.clear();
        }

        let header = apply_item_started(&mut live, &item);
        assert!(header.is_empty());

        assert!(apply_item_delta(
            &mut live,
            &item,
            TalosTimelineDelta::ToolArguments {
                arguments_delta: r#"{"file_path":"/tmp/delta.rs"}"#.to_string(),
            },
        )
        .is_empty());
        let result = apply_item_delta(
            &mut live,
            &item,
            TalosTimelineDelta::ToolResult {
                content: vec![TalosContentItem::FileRead {
                    path: "/tmp/delta.rs".to_string(),
                    offset: 0,
                    limit: 1,
                    total_lines: 1,
                    content: "ok".to_string(),
                    truncated: false,
                }],
                is_error: false,
            },
        );

        assert_eq!(
            result[0].lines[0].plain_text(),
            "● Read(/tmp/delta.rs) completed"
        );
        assert_eq!(
            result[0].lines[1].plain_text(),
            "  ⎿ Read 1 lines (ctrl+o to expand)"
        );
    }
}
