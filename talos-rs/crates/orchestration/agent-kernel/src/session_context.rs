use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use talos_error::TalosResult;
use talos_prompt::CompactPromptCursor;
use talos_store::session::{
    RecordHeader, SessionArtifactStore, SessionTurnWindow, SummaryRecord, TalosTurn,
};
use talos_types::talos::{RunItem, TalosContentItem};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TalosMessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TalosMessage {
    pub header: RecordHeader,
    pub role: TalosMessageRole,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionToolResultContext {
    pub header: RecordHeader,
    pub tool_name: String,
    pub content: String,
    pub is_error: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionToolExchangeOutcome {
    Succeeded,
    Failed,
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionToolExchangeContext {
    pub header: RecordHeader,
    pub call_id: String,
    pub tool_name: String,
    pub arguments_preview: String,
    pub outcome: SessionToolExchangeOutcome,
    pub output_preview: Option<String>,
    pub output_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SessionPromptContext {
    pub summaries: Vec<SummaryRecord>,
    pub recent_messages: Vec<TalosMessage>,
    pub recent_tool_results: Vec<SessionToolResultContext>,
    pub recent_tool_exchanges: Vec<SessionToolExchangeContext>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionPromptRecentWindow {
    #[default]
    Latest,
    OldestUncovered,
}

#[derive(Debug, Clone)]
pub struct SessionPromptContextConfig {
    pub compact_cursor: Option<CompactPromptCursor>,
    pub recent_window: SessionPromptRecentWindow,
    pub recent_messages_limit: usize,
    pub recent_tool_results_limit: usize,
    pub tool_output_max_chars: usize,
    pub recent_tool_exchanges_limit: usize,
    pub tool_exchange_max_chars: usize,
    pub include_summary: bool,
    pub summary_recent_count: usize,
    pub summary_max_chars: usize,
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if max_chars == 0 || value.chars().count() <= max_chars {
        value.to_string()
    } else {
        value.chars().take(max_chars).collect()
    }
}

fn select_window<T>(mut values: Vec<T>, limit: usize, window: SessionPromptRecentWindow) -> Vec<T> {
    if values.len() <= limit {
        return values;
    }
    match window {
        SessionPromptRecentWindow::Latest => values.split_off(values.len().saturating_sub(limit)),
        SessionPromptRecentWindow::OldestUncovered => {
            values.truncate(limit);
            values
        }
    }
}

fn messages_from_turn(turn: &TalosTurn) -> Vec<TalosMessage> {
    turn.items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            RunItem::UserInput { text, .. } => Some((index, TalosMessageRole::User, text)),
            RunItem::OutputText { text } => Some((index, TalosMessageRole::Assistant, text)),
            _ => None,
        })
        .map(|(index, role, text)| TalosMessage {
            header: RecordHeader {
                id: format!(
                    "{}:{}:{index}",
                    turn.turn_id,
                    match role {
                        TalosMessageRole::User => "user",
                        TalosMessageRole::Assistant => "assistant",
                        TalosMessageRole::System => "system",
                    }
                ),
                session_id: turn.session_id.clone(),
                turn_id: Some(turn.turn_id.clone()),
                timestamp: turn.timestamp,
            },
            role,
            text: text.clone(),
        })
        .collect()
}

fn render_content(content: &[TalosContentItem]) -> String {
    content
        .iter()
        .map(TalosContentItem::display_text)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn tool_results_from_turn(turn: &TalosTurn, max_chars: usize) -> Vec<SessionToolResultContext> {
    turn.items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            RunItem::ToolResult {
                tool_name,
                content,
                is_error,
                ..
            } => {
                let content = render_content(content);
                (!content.is_empty()).then(|| SessionToolResultContext {
                    header: RecordHeader {
                        id: format!("{}:tool_result:{index}", turn.turn_id),
                        session_id: turn.session_id.clone(),
                        turn_id: Some(turn.turn_id.clone()),
                        timestamp: turn.timestamp,
                    },
                    tool_name: tool_name.clone(),
                    truncated: max_chars > 0 && content.chars().count() > max_chars,
                    content: truncate_chars(&content, max_chars),
                    is_error: *is_error,
                })
            }
            _ => None,
        })
        .collect()
}

fn tool_exchanges_from_turn(turn: &TalosTurn, max_chars: usize) -> Vec<SessionToolExchangeContext> {
    let results = turn
        .items
        .iter()
        .filter_map(|item| match item {
            RunItem::ToolResult {
                call_id,
                content,
                is_error,
                ..
            } => Some((call_id.as_str(), (render_content(content), *is_error))),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    turn.items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            RunItem::ToolCall {
                call_id,
                name,
                arguments,
            } => {
                let result = results.get(call_id.as_str());
                let output = result.map(|(content, _)| truncate_chars(content, max_chars));
                Some(SessionToolExchangeContext {
                    header: RecordHeader {
                        id: format!("{}:tool_exchange:{index}", turn.turn_id),
                        session_id: turn.session_id.clone(),
                        turn_id: Some(turn.turn_id.clone()),
                        timestamp: turn.timestamp,
                    },
                    call_id: call_id.clone(),
                    tool_name: name.clone(),
                    arguments_preview: truncate_chars(&arguments.to_string(), max_chars),
                    outcome: match result {
                        Some((_, true)) => SessionToolExchangeOutcome::Failed,
                        Some((_, false)) => SessionToolExchangeOutcome::Succeeded,
                        None => SessionToolExchangeOutcome::Pending,
                    },
                    output_truncated: result.is_some_and(|(content, _)| {
                        max_chars > 0 && content.chars().count() > max_chars
                    }),
                    output_preview: output.filter(|value| !value.trim().is_empty()),
                })
            }
            _ => None,
        })
        .collect()
}

/// Builds the bounded session context consumed by prompt rendering and compaction.
pub async fn build_session_prompt_context(
    store: &dyn SessionArtifactStore,
    workspace_root: &Path,
    session_id: &str,
    config: &SessionPromptContextConfig,
) -> TalosResult<SessionPromptContext> {
    let turn_limit = config
        .recent_messages_limit
        .max(config.recent_tool_results_limit)
        .max(config.recent_tool_exchanges_limit);
    let window = match config.recent_window {
        SessionPromptRecentWindow::Latest => SessionTurnWindow::Latest,
        SessionPromptRecentWindow::OldestUncovered => SessionTurnWindow::OldestAfter {
            seq: config
                .compact_cursor
                .map(|cursor| cursor.covers_until_seq)
                .unwrap_or_default(),
        },
    };
    let turns = store
        .load_turn_window(workspace_root, session_id, window, turn_limit)
        .await?;
    let mut context = build_recent_session_context(&turns, config);
    context.summaries = if config.include_summary {
        store
            .load_summaries(workspace_root, session_id, config.summary_recent_count)
            .await?
    } else {
        Vec::new()
    };
    for summary in &mut context.summaries {
        summary.content = truncate_chars(&summary.content, config.summary_max_chars);
    }
    Ok(context)
}

pub(crate) fn build_recent_session_context(
    turns: &[TalosTurn],
    config: &SessionPromptContextConfig,
) -> SessionPromptContext {
    let uncovered = |turn: &&TalosTurn| {
        config
            .compact_cursor
            .is_none_or(|cursor| turn.seq > cursor.covers_until_seq)
    };
    let mut recent_messages = turns
        .iter()
        .filter(uncovered)
        .flat_map(messages_from_turn)
        .collect::<Vec<_>>();
    if config.compact_cursor.is_some()
        && config.recent_window == SessionPromptRecentWindow::Latest
        && recent_messages.len() < config.recent_messages_limit
    {
        let missing = config.recent_messages_limit - recent_messages.len();
        let mut backfill = turns
            .iter()
            .filter(|turn| !uncovered(turn))
            .flat_map(messages_from_turn)
            .collect::<Vec<_>>();
        if backfill.len() > missing {
            backfill = backfill.split_off(backfill.len() - missing);
        }
        backfill.extend(recent_messages);
        recent_messages = backfill;
    }
    recent_messages = select_window(
        recent_messages,
        config.recent_messages_limit,
        config.recent_window,
    );

    let mut recent_tool_results = turns
        .iter()
        .filter(uncovered)
        .flat_map(|turn| tool_results_from_turn(turn, config.tool_output_max_chars))
        .collect::<Vec<_>>();
    recent_tool_results = select_window(
        recent_tool_results,
        config.recent_tool_results_limit,
        config.recent_window,
    );
    let recent_tool_exchanges = select_window(
        turns
            .iter()
            .filter(uncovered)
            .flat_map(|turn| tool_exchanges_from_turn(turn, config.tool_exchange_max_chars))
            .collect(),
        config.recent_tool_exchanges_limit,
        config.recent_window,
    );
    SessionPromptContext {
        summaries: Vec::new(),
        recent_messages,
        recent_tool_results,
        recent_tool_exchanges,
    }
}

#[cfg(test)]
#[path = "session_context_tests.rs"]
mod tests;
