//! Canonical Agent message adapter for the render runtime live reducer.

use talos_sdk::{
    AgentAssistantTextPhase, AgentMessageBlock, AgentMessageBlockContent,
    AgentMessageBlockDeltaContent, AgentToolCallArguments, AgentUserContent,
};

use crate::render_runtime::common::style::StyledLine;
use crate::render_runtime::render::live::{LiveReducer, LiveTextKind};
use crate::render_runtime::render::user_input_display;
use crate::render_runtime::transcript::block::{FinalizedTranscriptBlock, TranscriptBlockKind};

/// Applies one canonical block-started event to render runtime live state.
pub(crate) fn apply_block_started(
    live: &mut LiveReducer,
    block: &AgentMessageBlock,
) -> Vec<FinalizedTranscriptBlock> {
    match &block.content {
        AgentMessageBlockContent::UserContent(content) => text_finalized_block(
            TranscriptBlockKind::UserInput,
            &user_content_display(content),
        ),
        AgentMessageBlockContent::Reasoning(reasoning) => {
            live.start_text(&block.block_id, LiveTextKind::Thinking, &reasoning.text);
            Vec::new()
        }
        AgentMessageBlockContent::AssistantText(assistant) => {
            let (kind, resolved) = assistant_text_state(assistant.phase);
            if resolved {
                live.start_text(&block.block_id, kind, &assistant.text);
            } else {
                live.start_unresolved_text(&block.block_id, kind, &assistant.text);
            }
            Vec::new()
        }
        AgentMessageBlockContent::ToolCall(tool) => {
            live.start_tool_with_arguments(
                &block.block_id,
                &tool.call_id,
                &tool.name,
                tool_arguments_text(&tool.arguments),
            );
            for progress in &tool.progress {
                live.append_tool_progress_delta(&block.block_id, progress.kind, &progress.text);
            }
            Vec::new()
        }
        AgentMessageBlockContent::SystemNotice(notice) => {
            text_finalized_block(TranscriptBlockKind::SystemNotice, notice.message())
        }
        AgentMessageBlockContent::ToolResult(_)
        | AgentMessageBlockContent::ExecutionSummary(_)
        | AgentMessageBlockContent::ContextCompaction(_) => Vec::new(),
    }
}

/// Applies one typed canonical block delta.
pub(crate) fn apply_block_delta(
    live: &mut LiveReducer,
    block: &AgentMessageBlock,
    delta: AgentMessageBlockDeltaContent,
) -> Vec<FinalizedTranscriptBlock> {
    match delta {
        AgentMessageBlockDeltaContent::Text(text) => {
            live.append_text(&block.block_id, &text);
            Vec::new()
        }
        AgentMessageBlockDeltaContent::AssistantTextPhase(phase) => {
            let (kind, _) = assistant_text_state(phase);
            live.commit_text(&block.block_id, kind)
                .into_iter()
                .collect()
        }
        AgentMessageBlockDeltaContent::ToolCallArguments(arguments) => {
            live.append_tool_arguments(&block.block_id, &arguments.json_fragment);
            Vec::new()
        }
        AgentMessageBlockDeltaContent::ToolProgress(progress) => {
            live.append_tool_progress_delta(&block.block_id, progress.kind, &progress.text);
            Vec::new()
        }
        AgentMessageBlockDeltaContent::ContextCompaction(_) => Vec::new(),
    }
}

/// Applies one completed canonical block snapshot.
pub(crate) fn apply_block_completed(
    live: &mut LiveReducer,
    block: &AgentMessageBlock,
    tool_call_block_id: Option<&str>,
) -> Vec<FinalizedTranscriptBlock> {
    match &block.content {
        AgentMessageBlockContent::Reasoning(reasoning) => {
            live.replace_text(&block.block_id, reasoning.text.clone());
            live.complete_text(&block.block_id).into_iter().collect()
        }
        AgentMessageBlockContent::AssistantText(assistant) => {
            let (kind, _) = assistant_text_state(assistant.phase);
            live.commit_text(&block.block_id, kind);
            live.replace_text(&block.block_id, assistant.text.clone());
            live.complete_text(&block.block_id).into_iter().collect()
        }
        AgentMessageBlockContent::ToolResult(result) => tool_call_block_id
            .and_then(|block_id| {
                live.complete_tool(block_id, result.is_error, result.content.clone())
            })
            .into_iter()
            .collect(),
        AgentMessageBlockContent::ExecutionSummary(_) => Vec::new(),
        AgentMessageBlockContent::ContextCompaction(compaction) => text_finalized_block(
            TranscriptBlockKind::SystemNotice,
            compaction
                .message
                .as_deref()
                .unwrap_or("Context compaction completed"),
        ),
        AgentMessageBlockContent::SystemNotice(notice) => {
            text_finalized_block(TranscriptBlockKind::SystemNotice, notice.message())
        }
        AgentMessageBlockContent::UserContent(_) | AgentMessageBlockContent::ToolCall(_) => {
            Vec::new()
        }
    }
}

fn assistant_text_state(phase: AgentAssistantTextPhase) -> (LiveTextKind, bool) {
    match phase {
        AgentAssistantTextPhase::Pending => (LiveTextKind::Commentary, false),
        AgentAssistantTextPhase::Commentary => (LiveTextKind::Commentary, true),
        AgentAssistantTextPhase::FinalAnswer => (LiveTextKind::FinalAnswer, true),
    }
}

fn tool_arguments_text(arguments: &AgentToolCallArguments) -> String {
    match arguments {
        AgentToolCallArguments::Streaming { json_fragment } => json_fragment.clone(),
        AgentToolCallArguments::Complete { value } => serde_json::to_string(value)
            .unwrap_or_else(|error| format!("<invalid tool arguments: {error}>")),
    }
}

fn user_content_display(content: &AgentUserContent) -> String {
    match content {
        AgentUserContent::Text { text } => text.clone(),
        AgentUserContent::SkillSelection { name, .. } => format!("[skill:{name}]"),
        AgentUserContent::File { file_id } => user_input_display("", std::slice::from_ref(file_id)),
        AgentUserContent::Audio { .. } => "[audio]".to_string(),
        AgentUserContent::Video { .. } => "[video]".to_string(),
        AgentUserContent::UserActionResponse { action_id, .. } => {
            format!("[user action response:{action_id}]")
        }
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
#[path = "timeline_tests.rs"]
mod tests;
