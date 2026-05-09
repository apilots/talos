use std::collections::{HashMap, HashSet, VecDeque};

use talos_error::{TalosError, TalosResult};
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::path_policy;
use crate::store::{
    MemoryStore, SessionPromptContext, SessionPromptContextConfig, SessionToolExchangeContext,
    SessionToolExchangeOutcome, SessionToolResultContext, SessionTranscriptBranch,
    SessionTranscriptPayload, SessionTranscriptRecord, SummaryRecord, TalosMessage, TalosTurn,
};
use crate::{RecordHeader, TalosMessageRole};
use talos_llm_provider::Role;
use talos_types::talos::{StopReason, TalosContentItem, TalosOutputItem, TalosUsage};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SessionArtifactRecord {
    Turn { turn: TalosTurn },
    Transcript { record: SessionTranscriptRecord },
    Summary { record: SummaryRecord },
}

impl SessionArtifactRecord {
    fn timestamp(&self) -> chrono::DateTime<chrono::Utc> {
        match self {
            Self::Turn { turn } => turn.timestamp,
            Self::Transcript { record } => record.header.timestamp,
            Self::Summary { record } => record.header.timestamp,
        }
    }
}

fn session_day_file_path(
    store: &MemoryStore,
    session_id: &str,
    timestamp: chrono::DateTime<chrono::Utc>,
) -> std::path::PathBuf {
    path_policy::session_dir(store, session_id)
        .join(format!("{}.jsonl", timestamp.format("%Y-%m-%d")))
}

fn serialize_session_artifact_line(record: &SessionArtifactRecord) -> TalosResult<String> {
    let payload = serde_json::to_string(record).map_err(|e| {
        TalosError::SerializationError(format!("serialize session artifact: {}", e))
    })?;
    Ok(format!("{payload}\n"))
}

async fn append_session_artifact(
    store: &MemoryStore,
    session_id: &str,
    timestamp: chrono::DateTime<chrono::Utc>,
    record: &SessionArtifactRecord,
) -> TalosResult<()> {
    let session_dir = path_policy::session_dir(store, session_id);
    fs::create_dir_all(&session_dir)
        .await
        .map_err(|e| TalosError::StorageError(format!("create session memory dir: {}", e)))?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(session_day_file_path(store, session_id, timestamp))
        .await
        .map_err(|e| TalosError::StorageError(format!("open session artifact file: {}", e)))?;

    let payload = serialize_session_artifact_line(record)?;
    file.write_all(payload.as_bytes())
        .await
        .map_err(|e| TalosError::StorageError(format!("append session artifact file: {}", e)))?;

    Ok(())
}

async fn load_session_artifacts(
    store: &MemoryStore,
    session_id: &str,
) -> TalosResult<Vec<SessionArtifactRecord>> {
    let mut files = path_policy::list_daily_files(store, session_id).await?;
    files.sort();

    let mut records = Vec::new();
    for path in files {
        let file = fs::File::open(&path)
            .await
            .map_err(|e| TalosError::StorageError(format!("open session artifact file: {}", e)))?;
        let mut lines = BufReader::new(file).lines();
        let mut line_number = 0_usize;

        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|e| TalosError::StorageError(format!("read session artifact file: {}", e)))?
        {
            line_number = line_number.saturating_add(1);
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let record: SessionArtifactRecord = serde_json::from_str(trimmed).map_err(|e| {
                TalosError::SerializationError(format!(
                    "parse session artifact {}:{}: {}",
                    path.display(),
                    line_number,
                    e
                ))
            })?;

            let record_session_id = match &record {
                SessionArtifactRecord::Turn { turn } => &turn.session_id,
                SessionArtifactRecord::Transcript { record } => &record.header.session_id,
                SessionArtifactRecord::Summary { record } => &record.header.session_id,
            };
            if record_session_id != session_id {
                return Err(TalosError::ValidationError(format!(
                    "session artifact {}:{} belongs to session {}, expected {}",
                    path.display(),
                    line_number,
                    record_session_id,
                    session_id
                )));
            }

            records.push(record);
        }
    }

    records.sort_by_key(SessionArtifactRecord::timestamp);
    Ok(records)
}

pub(crate) async fn load_turns(
    store: &MemoryStore,
    session_id: &str,
) -> TalosResult<Vec<TalosTurn>> {
    let artifacts = load_session_artifacts(store, session_id).await?;
    let mut turns = artifacts
        .iter()
        .filter_map(|record| match record {
            SessionArtifactRecord::Turn { turn } => Some(turn.clone()),
            SessionArtifactRecord::Transcript { .. } | SessionArtifactRecord::Summary { .. } => {
                None
            }
        })
        .collect::<Vec<_>>();

    turns.extend(synthesize_cancelled_turns_from_transcripts(
        session_id, &artifacts, &turns,
    ));

    turns.sort_by(|a, b| a.seq.cmp(&b.seq).then(a.timestamp.cmp(&b.timestamp)));
    Ok(turns)
}

#[derive(Debug, Clone)]
struct InterruptedTranscriptGroup {
    invocation_id: String,
    timestamp: chrono::DateTime<chrono::Utc>,
    user_text: Option<String>,
    has_agent_response: bool,
}

fn synthesize_cancelled_turns_from_transcripts(
    session_id: &str,
    artifacts: &[SessionArtifactRecord],
    canonical_turns: &[TalosTurn],
) -> Vec<TalosTurn> {
    let canonical_turn_ids = canonical_turns
        .iter()
        .map(|turn| turn.turn_id.as_str())
        .collect::<HashSet<_>>();
    let canonical_user_inputs = canonical_turns
        .iter()
        .flat_map(|turn| {
            turn.items.iter().filter_map(move |item| match item {
                TalosOutputItem::UserInput { text } => Some((text.as_str(), turn.timestamp)),
                TalosOutputItem::Reasoning { .. }
                | TalosOutputItem::ExecutionSummary { .. }
                | TalosOutputItem::OutputText { .. }
                | TalosOutputItem::ToolCall { .. }
                | TalosOutputItem::ToolResult { .. } => None,
            })
        })
        .collect::<Vec<_>>();
    let mut groups: HashMap<String, InterruptedTranscriptGroup> = HashMap::new();

    for artifact in artifacts {
        let SessionArtifactRecord::Transcript { record } = artifact else {
            continue;
        };
        if canonical_turn_ids.contains(record.header.invocation_id.as_str()) {
            continue;
        }
        let group = groups
            .entry(record.header.invocation_id.clone())
            .or_insert_with(|| InterruptedTranscriptGroup {
                invocation_id: record.header.invocation_id.clone(),
                timestamp: record.header.timestamp,
                user_text: None,
                has_agent_response: false,
            });
        if record.header.timestamp < group.timestamp {
            group.timestamp = record.header.timestamp;
        }

        let SessionTranscriptPayload::Message { message } = &record.payload else {
            continue;
        };
        let text = message.content.trim();
        if text.is_empty() {
            continue;
        }
        match message.role {
            Role::User if group.user_text.is_none() => {
                group.user_text = Some(text.to_string());
                group.timestamp = record.header.timestamp;
            }
            Role::Assistant => {
                group.has_agent_response = true;
            }
            Role::System | Role::Tool | Role::User => {}
        }
    }

    let next_seq = canonical_turns
        .iter()
        .map(|turn| turn.seq)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let mut groups = groups
        .into_values()
        .filter(|group| group.user_text.is_some() && !group.has_agent_response)
        .collect::<Vec<_>>();
    groups.sort_by_key(|group| group.timestamp);

    groups
        .into_iter()
        .enumerate()
        .filter_map(|(index, group)| {
            let user_text = group.user_text?;
            let represented_by_canonical_turn =
                canonical_user_inputs
                    .iter()
                    .any(|(canonical_text, turn_timestamp)| {
                        *canonical_text == user_text && *turn_timestamp >= group.timestamp
                    });
            if represented_by_canonical_turn {
                return None;
            }
            Some(TalosTurn {
                turn_id: group.invocation_id,
                message_id: String::new(),
                session_id: session_id.to_string(),
                seq: next_seq.saturating_add(index as u64),
                timestamp: group.timestamp,
                items: vec![TalosOutputItem::UserInput { text: user_text }],
                usage: TalosUsage::default(),
                stop_reason: StopReason::Cancelled,
            })
        })
        .collect()
}

pub(crate) async fn load_transcript_records(
    store: &MemoryStore,
    session_id: &str,
    invocation_id: Option<&str>,
    branch: SessionTranscriptBranch,
) -> TalosResult<Vec<SessionTranscriptRecord>> {
    let mut records = load_session_artifacts(store, session_id)
        .await?
        .into_iter()
        .filter_map(|entry| match entry {
            SessionArtifactRecord::Transcript { record }
                if invocation_id
                    .map(|expected| record.header.invocation_id == expected)
                    .unwrap_or(true)
                    && record.header.branch == branch =>
            {
                Some(record)
            }
            SessionArtifactRecord::Turn { .. }
            | SessionArtifactRecord::Summary { .. }
            | SessionArtifactRecord::Transcript { .. } => None,
        })
        .collect::<Vec<_>>();

    records.sort_by(|a, b| a.header.timestamp.cmp(&b.header.timestamp));
    Ok(records)
}

async fn load_all_transcript_records(
    store: &MemoryStore,
    session_id: &str,
) -> TalosResult<Vec<SessionTranscriptRecord>> {
    let mut records = load_session_artifacts(store, session_id)
        .await?
        .into_iter()
        .filter_map(|entry| match entry {
            SessionArtifactRecord::Transcript { record } => Some(record),
            SessionArtifactRecord::Turn { .. } | SessionArtifactRecord::Summary { .. } => None,
        })
        .collect::<Vec<_>>();

    records.sort_by(|a, b| a.header.timestamp.cmp(&b.header.timestamp));
    Ok(records)
}

fn dedupe_keep_latest(messages: Vec<TalosMessage>) -> Vec<TalosMessage> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for message in messages.into_iter().rev() {
        if seen.insert(message.header.id.clone()) {
            out.push(message);
        }
    }
    out.reverse();
    out
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    input.chars().take(max_chars).collect::<String>()
}

fn render_tool_result_content(content: &[TalosContentItem]) -> String {
    content
        .iter()
        .map(|item| match item {
            TalosContentItem::Text { text } => text.trim().to_string(),
            TalosContentItem::Json { value } => value.to_string(),
        })
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn recent_messages_from_turn(turn: &TalosTurn) -> Vec<TalosMessage> {
    turn.items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            TalosOutputItem::UserInput { text } => Some(TalosMessage {
                header: RecordHeader {
                    id: format!("{}:user:{}", turn.turn_id, index),
                    session_id: turn.session_id.clone(),
                    turn_id: Some(turn.turn_id.clone()),
                    timestamp: turn.timestamp,
                },
                role: TalosMessageRole::User,
                text: text.clone(),
            }),
            TalosOutputItem::OutputText { text } => Some(TalosMessage {
                header: RecordHeader {
                    id: format!("{}:assistant:{}", turn.turn_id, index),
                    session_id: turn.session_id.clone(),
                    turn_id: Some(turn.turn_id.clone()),
                    timestamp: turn.timestamp,
                },
                role: TalosMessageRole::Assistant,
                text: text.clone(),
            }),
            TalosOutputItem::Reasoning { .. }
            | TalosOutputItem::ExecutionSummary { .. }
            | TalosOutputItem::ToolCall { .. }
            | TalosOutputItem::ToolResult { .. } => None,
        })
        .collect()
}

fn transcript_message_role(role: Role) -> Option<TalosMessageRole> {
    match role {
        Role::System => Some(TalosMessageRole::System),
        Role::User => Some(TalosMessageRole::User),
        Role::Assistant => Some(TalosMessageRole::Assistant),
        Role::Tool => None,
    }
}

fn recent_message_from_transcript(record: &SessionTranscriptRecord) -> Option<TalosMessage> {
    let SessionTranscriptPayload::Message { message } = &record.payload else {
        return None;
    };
    let role = transcript_message_role(message.role)?;
    let text = message.content.trim();
    if text.is_empty() {
        return None;
    }

    Some(TalosMessage {
        header: RecordHeader {
            id: record.header.id.clone(),
            session_id: record.header.session_id.clone(),
            turn_id: Some(record.header.invocation_id.clone()),
            timestamp: record.header.timestamp,
        },
        role,
        text: text.to_string(),
    })
}

fn recent_tool_results_from_turn(
    turn: &TalosTurn,
    max_chars: usize,
) -> Vec<SessionToolResultContext> {
    turn.items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            TalosOutputItem::ToolResult {
                tool_name,
                content,
                is_error,
                ..
            } => {
                let rendered = render_tool_result_content(content);
                if rendered.is_empty() {
                    return None;
                }
                let truncated = max_chars > 0 && rendered.chars().count() > max_chars;
                Some(SessionToolResultContext {
                    header: RecordHeader {
                        id: format!("{}:tool_result:{}", turn.turn_id, index),
                        session_id: turn.session_id.clone(),
                        turn_id: Some(turn.turn_id.clone()),
                        timestamp: turn.timestamp,
                    },
                    tool_name: tool_name.clone(),
                    content: if max_chars > 0 {
                        truncate_chars(&rendered, max_chars)
                    } else {
                        rendered
                    },
                    is_error: *is_error,
                    truncated,
                })
            }
            TalosOutputItem::UserInput { .. }
            | TalosOutputItem::Reasoning { .. }
            | TalosOutputItem::ExecutionSummary { .. }
            | TalosOutputItem::OutputText { .. }
            | TalosOutputItem::ToolCall { .. } => None,
        })
        .collect()
}

fn recent_tool_result_from_transcript(
    record: &SessionTranscriptRecord,
    max_chars: usize,
) -> Option<SessionToolResultContext> {
    let SessionTranscriptPayload::Message { message } = &record.payload else {
        return None;
    };
    if message.role != Role::Tool {
        return None;
    }
    let text = message.content.trim();
    if text.is_empty() {
        return None;
    }

    Some(SessionToolResultContext {
        header: RecordHeader {
            id: record.header.id.clone(),
            session_id: record.header.session_id.clone(),
            turn_id: Some(record.header.invocation_id.clone()),
            timestamp: record.header.timestamp,
        },
        tool_name: message.name.clone().unwrap_or_else(|| "Tool".to_string()),
        content: if max_chars > 0 {
            truncate_chars(text, max_chars)
        } else {
            text.to_string()
        },
        is_error: false,
        truncated: max_chars > 0 && text.chars().count() > max_chars,
    })
}

fn recent_tool_exchanges_from_turn(
    turn: &TalosTurn,
    max_chars: usize,
) -> Vec<SessionToolExchangeContext> {
    let tool_results = turn
        .items
        .iter()
        .filter_map(|item| match item {
            TalosOutputItem::ToolResult {
                call_id,
                tool_name,
                content,
                is_error,
            } => {
                let rendered = render_tool_result_content(content);
                let output_truncated = max_chars > 0 && rendered.chars().count() > max_chars;
                Some((
                    call_id.clone(),
                    (
                        tool_name.clone(),
                        if max_chars > 0 {
                            truncate_chars(&rendered, max_chars)
                        } else {
                            rendered
                        },
                        *is_error,
                        output_truncated,
                    ),
                ))
            }
            _ => None,
        })
        .collect::<std::collections::HashMap<_, _>>();

    turn.items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            TalosOutputItem::ToolCall {
                call_id,
                name,
                arguments,
            } => {
                let arguments_rendered = arguments.to_string();
                let (outcome, output_preview, output_truncated) = tool_results
                    .get(call_id)
                    .map(|(_tool_name, output_preview, is_error, output_truncated)| {
                        (
                            if *is_error {
                                SessionToolExchangeOutcome::Failed
                            } else {
                                SessionToolExchangeOutcome::Succeeded
                            },
                            (!output_preview.trim().is_empty()).then_some(output_preview.clone()),
                            *output_truncated,
                        )
                    })
                    .unwrap_or((SessionToolExchangeOutcome::Pending, None, false));
                Some(SessionToolExchangeContext {
                    header: RecordHeader {
                        id: format!("{}:tool_exchange:{}", turn.turn_id, index),
                        session_id: turn.session_id.clone(),
                        turn_id: Some(turn.turn_id.clone()),
                        timestamp: turn.timestamp,
                    },
                    call_id: call_id.clone(),
                    tool_name: name.clone(),
                    arguments_preview: if max_chars > 0 {
                        truncate_chars(&arguments_rendered, max_chars)
                    } else {
                        arguments_rendered
                    },
                    outcome,
                    output_preview,
                    output_truncated,
                })
            }
            _ => None,
        })
        .collect()
}

pub(crate) async fn remember_hot_messages(store: &MemoryStore, turn: &TalosTurn) {
    let mut cache = store.hot_cache.write().await;
    let entry = cache
        .entry(turn.session_id.clone())
        .or_insert_with(VecDeque::new);
    for message in recent_messages_from_turn(turn) {
        entry.push_back(message);
    }
    while entry.len() > store.hot_recent_messages {
        entry.pop_front();
    }
}

pub(crate) async fn next_turn_seq(store: &MemoryStore, session_id: &str) -> TalosResult<u64> {
    {
        let mut seq_cache = store.seq_cache.write().await;
        if let Some(next) = seq_cache.get_mut(session_id) {
            let seq = *next;
            *next = next.saturating_add(1);
            return Ok(seq);
        }
    }

    let max_seq = latest_seq(store, session_id).await?;
    let seq = max_seq.saturating_add(1);
    let mut seq_cache = store.seq_cache.write().await;
    seq_cache.insert(session_id.to_string(), seq.saturating_add(1));
    Ok(seq)
}

pub(crate) async fn append_turn(store: &MemoryStore, turn: &TalosTurn) -> TalosResult<()> {
    if turn.turn_id.trim().is_empty() {
        return Err(TalosError::ValidationError(
            "canonical turn turn_id cannot be empty".to_string(),
        ));
    }
    if turn.session_id.trim().is_empty() {
        return Err(TalosError::ValidationError(
            "canonical turn session_id cannot be empty".to_string(),
        ));
    }
    if turn.message_id.trim().is_empty() {
        return Err(TalosError::ValidationError(
            "canonical turn message_id cannot be empty".to_string(),
        ));
    }

    append_session_artifact(
        store,
        &turn.session_id,
        turn.timestamp,
        &SessionArtifactRecord::Turn { turn: turn.clone() },
    )
    .await?;

    remember_hot_messages(store, turn).await;

    {
        let mut seq_cache = store.seq_cache.write().await;
        let next = turn.seq.saturating_add(1);
        let entry = seq_cache.entry(turn.session_id.clone()).or_insert(next);
        if *entry < next {
            *entry = next;
        }
    }

    Ok(())
}

pub(crate) async fn append_transcript_record(
    store: &MemoryStore,
    record: &SessionTranscriptRecord,
) -> TalosResult<()> {
    if record.header.id.trim().is_empty() {
        return Err(TalosError::ValidationError(
            "transcript record id cannot be empty".to_string(),
        ));
    }
    if record.header.session_id.trim().is_empty() {
        return Err(TalosError::ValidationError(
            "transcript record session_id cannot be empty".to_string(),
        ));
    }
    if record.header.invocation_id.trim().is_empty() {
        return Err(TalosError::ValidationError(
            "transcript record invocation_id cannot be empty".to_string(),
        ));
    }

    append_session_artifact(
        store,
        &record.header.session_id,
        record.header.timestamp,
        &SessionArtifactRecord::Transcript {
            record: record.clone(),
        },
    )
    .await?;

    let mut cache = store.transcript_cache.write().await;
    let entry = cache
        .entry(record.header.session_id.clone())
        .or_insert_with(VecDeque::new);
    entry.push_back(record.clone());
    while entry.len() > store.hot_recent_messages.saturating_mul(4) {
        entry.pop_front();
    }

    Ok(())
}

pub(crate) async fn build_session_prompt_context(
    store: &MemoryStore,
    session_id: &str,
    config: &SessionPromptContextConfig,
) -> TalosResult<SessionPromptContext> {
    let target_recent_messages = config.recent_messages_limit;
    let recent_messages = if target_recent_messages == 0 {
        Vec::new()
    } else {
        let hot = {
            let cache = store.hot_cache.read().await;
            cache
                .get(session_id)
                .map(|value| value.iter().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        };

        let hot = if hot.len() > target_recent_messages {
            hot[hot.len() - target_recent_messages..].to_vec()
        } else {
            hot
        };

        if hot.len() >= target_recent_messages {
            hot
        } else {
            let turns = load_turns(store, session_id).await?;
            let mut from_disk = turns
                .into_iter()
                .flat_map(|turn| recent_messages_from_turn(&turn))
                .collect::<Vec<_>>();
            if from_disk.is_empty() {
                from_disk = load_all_transcript_records(store, session_id)
                    .await?
                    .iter()
                    .filter_map(recent_message_from_transcript)
                    .collect::<Vec<_>>();
            }
            if from_disk.len() > target_recent_messages.saturating_mul(3) {
                let start = from_disk
                    .len()
                    .saturating_sub(target_recent_messages.saturating_mul(3));
                from_disk = from_disk.split_off(start);
            }
            from_disk.extend(hot);
            let deduped = dedupe_keep_latest(from_disk);
            if deduped.len() > target_recent_messages {
                deduped[deduped.len() - target_recent_messages..].to_vec()
            } else {
                deduped
            }
        }
    };

    let recent_tool_results = if config.recent_tool_results_limit == 0 {
        Vec::new()
    } else {
        let turns = load_turns(store, session_id).await?;
        let mut tool_results = turns
            .into_iter()
            .flat_map(|turn| recent_tool_results_from_turn(&turn, config.tool_output_max_chars))
            .collect::<Vec<_>>();
        if tool_results.is_empty() {
            tool_results = load_all_transcript_records(store, session_id)
                .await?
                .iter()
                .filter_map(|record| {
                    recent_tool_result_from_transcript(record, config.tool_output_max_chars)
                })
                .collect::<Vec<_>>();
        }
        if tool_results.len() > config.recent_tool_results_limit {
            let start = tool_results
                .len()
                .saturating_sub(config.recent_tool_results_limit);
            tool_results = tool_results.split_off(start);
        }
        tool_results
    };

    let recent_tool_exchanges = if config.recent_tool_exchanges_limit == 0 {
        Vec::new()
    } else {
        let turns = load_turns(store, session_id).await?;
        let mut tool_exchanges = turns
            .into_iter()
            .flat_map(|turn| recent_tool_exchanges_from_turn(&turn, config.tool_exchange_max_chars))
            .collect::<Vec<_>>();
        if tool_exchanges.len() > config.recent_tool_exchanges_limit {
            let start = tool_exchanges
                .len()
                .saturating_sub(config.recent_tool_exchanges_limit);
            tool_exchanges = tool_exchanges.split_off(start);
        }
        tool_exchanges
    };

    let summaries = if config.include_summary && config.summary_recent_count > 0 {
        let mut records = load_summaries(store, session_id, config.summary_recent_count).await?;
        if config.summary_max_chars > 0 {
            for record in &mut records {
                record.content = truncate_chars(&record.content, config.summary_max_chars);
            }
        }
        records
    } else {
        Vec::new()
    };

    Ok(SessionPromptContext {
        summaries,
        recent_messages,
        recent_tool_results,
        recent_tool_exchanges,
    })
}

pub(crate) async fn save_summary(store: &MemoryStore, record: &SummaryRecord) -> TalosResult<()> {
    if record.header.session_id.trim().is_empty() {
        return Err(TalosError::ValidationError(
            "summary session_id cannot be empty".to_string(),
        ));
    }

    append_session_artifact(
        store,
        &record.header.session_id,
        record.header.timestamp,
        &SessionArtifactRecord::Summary {
            record: record.clone(),
        },
    )
    .await
}

pub(crate) async fn load_summaries(
    store: &MemoryStore,
    session_id: &str,
    limit: usize,
) -> TalosResult<Vec<SummaryRecord>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let mut records = load_session_artifacts(store, session_id)
        .await?
        .into_iter()
        .filter_map(|entry| match entry {
            SessionArtifactRecord::Summary { record } => Some(record),
            SessionArtifactRecord::Turn { .. } | SessionArtifactRecord::Transcript { .. } => None,
        })
        .collect::<Vec<_>>();

    records.sort_by(|a, b| a.header.timestamp.cmp(&b.header.timestamp));
    if records.len() <= limit {
        return Ok(records);
    }

    Ok(records.split_off(records.len() - limit))
}

pub(crate) async fn latest_seq(store: &MemoryStore, session_id: &str) -> TalosResult<u64> {
    {
        let seq_cache = store.seq_cache.read().await;
        if let Some(next_seq) = seq_cache.get(session_id) {
            return Ok(next_seq.saturating_sub(1));
        }
    }

    let turns = load_turns(store, session_id).await?;
    let max_seq = turns.last().map(|turn| turn.seq).unwrap_or(0);
    Ok(max_seq)
}

pub(crate) async fn session_size_bytes(store: &MemoryStore, session_id: &str) -> TalosResult<u64> {
    let mut total = 0_u64;
    for path in path_policy::list_daily_files(store, session_id).await? {
        let metadata = fs::metadata(path)
            .await
            .map_err(|e| TalosError::StorageError(format!("read daily metadata: {}", e)))?;
        total = total.saturating_add(metadata.len());
    }

    Ok(total)
}

pub(crate) async fn reset_session(store: &MemoryStore, session_id: &str) -> TalosResult<()> {
    let session_dir = path_policy::session_dir(store, session_id);
    match fs::remove_dir_all(&session_dir).await {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(TalosError::StorageError(format!(
                "remove session memory dir: {}",
                err
            )));
        }
    }

    store.hot_cache.write().await.remove(session_id);
    store.transcript_cache.write().await.remove(session_id);
    store.seq_cache.write().await.remove(session_id);
    Ok(())
}
