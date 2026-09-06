use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use talos_agent::{SessionHistoryReadParams, SessionHistoryReader, SessionHistoryWindow};
use talos_agent_api::{
    AgentAssistantTextPhase, AgentMessage, AgentMessageBlockContent, AgentMessageRole,
    AgentRunResult, AgentToolCallArguments, AgentUserContent, SessionHistoryTurn,
};
use talos_agent_kernel::{
    KernelSessionArtifactStore, SessionPromptContext, SessionPromptContextConfig,
    SessionPromptContextFuture, SessionPromptContextSource, SessionPromptEntryHeader,
    SessionPromptMessage, SessionPromptMessageRole, SessionPromptRecentWindow,
    SessionPromptSourceTurn, SessionPromptSummary, SessionToolExchangeContext,
    SessionToolExchangeOutcome, SessionToolResultContext,
};
use talos_types::TalosContentItem;

/// Kernel-private projection source backed by canonical Talos Session history.
pub struct KernelSessionPromptContextSource {
    history: Arc<dyn SessionHistoryReader>,
    artifacts: Arc<dyn KernelSessionArtifactStore>,
    workspace_root: PathBuf,
    session_id: String,
}

impl KernelSessionPromptContextSource {
    pub fn new(
        history: Arc<dyn SessionHistoryReader>,
        artifacts: Arc<dyn KernelSessionArtifactStore>,
        workspace_root: PathBuf,
        session_id: String,
    ) -> Self {
        Self {
            history,
            artifacts,
            workspace_root,
            session_id,
        }
    }
}

impl SessionPromptContextSource for KernelSessionPromptContextSource {
    fn load(&self, config: SessionPromptContextConfig) -> SessionPromptContextFuture<'_> {
        Box::pin(async move {
            let turn_limit = config
                .recent_messages_limit
                .max(config.recent_tool_results_limit)
                .max(config.recent_tool_exchanges_limit);
            let turns = if turn_limit == 0 {
                Vec::new()
            } else {
                let window = match config.recent_window {
                    SessionPromptRecentWindow::Latest => SessionHistoryWindow::Latest,
                    SessionPromptRecentWindow::OldestUncovered => {
                        let completed_event_seq = config
                            .compact_cursor
                            .map(|cursor| i64::try_from(cursor.covers_until_seq))
                            .transpose()
                            .map_err(|_| {
                                talos_error::TalosError::internal(
                                    "Session prompt cursor exceeds canonical event sequence range",
                                )
                            })?
                            .unwrap_or_default();
                        SessionHistoryWindow::OldestAfter {
                            completed_event_seq,
                        }
                    }
                };
                self.history
                    .read(SessionHistoryReadParams {
                        session_id: self.session_id.clone(),
                        window,
                        limit: turn_limit,
                    })
                    .await
                    .map_err(|error| error.into_talos_error())?
            };
            let mut context = build_context(&turns, &config);
            context.summaries = if config.include_summary {
                self.artifacts
                    .load_summaries(
                        &self.workspace_root,
                        &self.session_id,
                        config.summary_recent_count,
                    )
                    .await?
                    .into_iter()
                    .map(|summary| SessionPromptSummary {
                        id: summary.header.id,
                        covers_until_seq: summary.covers_until_seq,
                        source_turn_count: summary.source_turn_count,
                        content: truncate_chars(&summary.content, config.summary_max_chars),
                    })
                    .collect()
            } else {
                Vec::new()
            };
            Ok(context)
        })
    }
}

fn build_context(
    turns: &[SessionHistoryTurn],
    config: &SessionPromptContextConfig,
) -> SessionPromptContext {
    let uncovered = |turn: &&SessionHistoryTurn| {
        config.compact_cursor.is_none_or(|cursor| {
            u64::try_from(turn.completed_event_seq)
                .is_ok_and(|sequence| sequence > cursor.covers_until_seq)
        })
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

    let recent_tool_results = select_window(
        turns
            .iter()
            .filter(uncovered)
            .flat_map(|turn| tool_results_from_turn(turn, config.tool_output_max_chars))
            .collect(),
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
        source_turns: turns
            .iter()
            .map(|turn| SessionPromptSourceTurn {
                turn_id: turn.identity.turn_id.clone(),
                completed_event_seq: turn.completed_event_seq,
                completed_at: turn.completed_at,
            })
            .collect(),
        recent_messages,
        recent_tool_results,
        recent_tool_exchanges,
    }
}

fn messages_from_turn(turn: &SessionHistoryTurn) -> Vec<SessionPromptMessage> {
    turn_messages(turn)
        .iter()
        .filter_map(|message| {
            let role = match message.role {
                AgentMessageRole::User => SessionPromptMessageRole::User,
                AgentMessageRole::Agent => SessionPromptMessageRole::Assistant,
                AgentMessageRole::System => SessionPromptMessageRole::System,
                AgentMessageRole::Tool => return None,
            };
            let text = message
                .blocks
                .iter()
                .filter_map(|block| match (&message.role, &block.content) {
                    (
                        AgentMessageRole::User,
                        AgentMessageBlockContent::UserContent(AgentUserContent::Text { text }),
                    ) => Some(text.as_str()),
                    (AgentMessageRole::Agent, AgentMessageBlockContent::AssistantText(text))
                        if matches!(
                            text.phase,
                            AgentAssistantTextPhase::Commentary
                                | AgentAssistantTextPhase::FinalAnswer
                        ) =>
                    {
                        Some(text.text.as_str())
                    }
                    (AgentMessageRole::System, AgentMessageBlockContent::SystemNotice(notice)) => {
                        Some(notice.message())
                    }
                    _ => None,
                })
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then(|| SessionPromptMessage {
                header: entry_header(
                    message.message_id.clone(),
                    &turn.identity.turn_id,
                    message.completed_at.unwrap_or(turn.completed_at),
                ),
                role,
                text,
            })
        })
        .collect()
}

fn tool_results_from_turn(
    turn: &SessionHistoryTurn,
    max_chars: usize,
) -> Vec<SessionToolResultContext> {
    turn_messages(turn)
        .iter()
        .flat_map(|message| &message.blocks)
        .filter_map(|block| match &block.content {
            AgentMessageBlockContent::ToolResult(result) => {
                let content = render_content(&result.content);
                (!content.is_empty()).then(|| SessionToolResultContext {
                    header: entry_header(
                        block.block_id.clone(),
                        &turn.identity.turn_id,
                        block.completed_at.unwrap_or(turn.completed_at),
                    ),
                    tool_name: result.tool_name.clone(),
                    truncated: max_chars > 0 && content.chars().count() > max_chars,
                    content: truncate_chars(&content, max_chars),
                    is_error: result.is_error,
                })
            }
            _ => None,
        })
        .collect()
}

fn tool_exchanges_from_turn(
    turn: &SessionHistoryTurn,
    max_chars: usize,
) -> Vec<SessionToolExchangeContext> {
    let results = turn_messages(turn)
        .iter()
        .flat_map(|message| &message.blocks)
        .filter_map(|block| match &block.content {
            AgentMessageBlockContent::ToolResult(result) => Some((
                result.call_id.as_str(),
                (render_content(&result.content), result.is_error),
            )),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    turn_messages(turn)
        .iter()
        .flat_map(|message| &message.blocks)
        .filter_map(|block| match &block.content {
            AgentMessageBlockContent::ToolCall(call) => {
                let arguments = match &call.arguments {
                    AgentToolCallArguments::Streaming { json_fragment } => json_fragment.clone(),
                    AgentToolCallArguments::Complete { value } => value.to_string(),
                };
                let result = results.get(call.call_id.as_str());
                Some(SessionToolExchangeContext {
                    header: entry_header(
                        block.block_id.clone(),
                        &turn.identity.turn_id,
                        block.completed_at.unwrap_or(turn.completed_at),
                    ),
                    call_id: call.call_id.clone(),
                    tool_name: call.name.clone(),
                    arguments_preview: truncate_chars(&arguments, max_chars),
                    outcome: match result {
                        Some((_, true)) => SessionToolExchangeOutcome::Failed,
                        Some((_, false)) => SessionToolExchangeOutcome::Succeeded,
                        None => SessionToolExchangeOutcome::Pending,
                    },
                    output_preview: result
                        .map(|(content, _)| truncate_chars(content, max_chars))
                        .filter(|content| !content.trim().is_empty()),
                    output_truncated: result.is_some_and(|(content, _)| {
                        max_chars > 0 && content.chars().count() > max_chars
                    }),
                })
            }
            _ => None,
        })
        .collect()
}

fn turn_messages(turn: &SessionHistoryTurn) -> &[AgentMessage] {
    match &turn.result {
        AgentRunResult::Completed(output) => &output.messages,
        AgentRunResult::Failed(failure) => &failure.messages,
    }
}

fn entry_header(id: String, turn_id: &str, completed_at: i64) -> SessionPromptEntryHeader {
    SessionPromptEntryHeader {
        id,
        turn_id: turn_id.to_string(),
        completed_at,
    }
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
        SessionPromptRecentWindow::Latest => values.split_off(values.len() - limit),
        SessionPromptRecentWindow::OldestUncovered => {
            values.truncate(limit);
            values
        }
    }
}
