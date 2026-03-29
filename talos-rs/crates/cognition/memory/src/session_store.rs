use std::collections::{HashSet, VecDeque};

use talos_error::{TalosError, TalosResult};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;

use crate::path_policy;
use crate::store::{
    MemoryStore, SessionPromptContext, SessionPromptContextConfig, SessionTranscriptBranch,
    SessionTranscriptRecord, SummaryRecord, TalosMessage, TalosTurn,
};

const TURN_V2_MARKER: &str = "```json turn_v2";
const SUMMARY_V2_MARKER: &str = "```json summary_v2";
const TRANSCRIPT_V1_MARKER: &str = "```json transcript_v1";

pub(crate) fn render_turn_block(turn: &TalosTurn) -> TalosResult<String> {
    let payload = serde_json::to_string_pretty(turn)
        .map_err(|e| TalosError::SerializationError(format!("serialize canonical turn: {}", e)))?;
    Ok(format!(
        "## Turn {}\n{}\n{}\n```\n\n",
        turn.turn_id, TURN_V2_MARKER, payload
    ))
}

pub(crate) fn render_summary_block(record: &SummaryRecord) -> TalosResult<String> {
    let payload = serde_json::to_string_pretty(record)
        .map_err(|e| TalosError::SerializationError(format!("serialize summary record: {}", e)))?;
    Ok(format!(
        "## Summary {}\n{}\n{}\n```\n\n",
        record.header.id, SUMMARY_V2_MARKER, payload
    ))
}

pub(crate) fn render_transcript_block(record: &SessionTranscriptRecord) -> TalosResult<String> {
    let payload = serde_json::to_string_pretty(record).map_err(|e| {
        TalosError::SerializationError(format!("serialize transcript record: {}", e))
    })?;
    Ok(format!(
        "## Transcript {}\n{}\n{}\n```\n\n",
        record.header.id, TRANSCRIPT_V1_MARKER, payload
    ))
}

pub(crate) fn parse_json_blocks(content: &str, marker: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut collecting = false;
    let mut buf = String::new();

    for line in content.lines() {
        if !collecting && line.trim() == marker {
            collecting = true;
            buf.clear();
            continue;
        }
        if collecting && line.trim() == "```" {
            if !buf.trim().is_empty() {
                blocks.push(buf.trim().to_string());
            }
            collecting = false;
            buf.clear();
            continue;
        }
        if collecting {
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(line);
        }
    }

    blocks
}

pub(crate) async fn load_turns(
    store: &MemoryStore,
    session_id: &str,
) -> TalosResult<Vec<TalosTurn>> {
    let mut files = path_policy::list_daily_files(store, session_id).await?;
    files.sort();

    let mut turns = Vec::new();
    for path in files {
        let content = fs::read_to_string(path)
            .await
            .map_err(|e| TalosError::StorageError(format!("read daily turn file: {}", e)))?;
        for block in parse_json_blocks(&content, TURN_V2_MARKER) {
            if let Ok(turn) = serde_json::from_str::<TalosTurn>(&block) {
                if turn.session_id == session_id {
                    turns.push(turn);
                }
            }
        }
    }

    turns.sort_by(|a, b| a.seq.cmp(&b.seq).then(a.timestamp.cmp(&b.timestamp)));
    Ok(turns)
}

pub(crate) async fn load_transcript_records(
    store: &MemoryStore,
    session_id: &str,
    invocation_id: Option<&str>,
    branch: SessionTranscriptBranch,
) -> TalosResult<Vec<SessionTranscriptRecord>> {
    let mut files = path_policy::list_daily_files(store, session_id).await?;
    files.sort();

    let mut records = Vec::new();
    for path in files {
        let content = fs::read_to_string(path)
            .await
            .map_err(|e| TalosError::StorageError(format!("read daily transcript file: {}", e)))?;
        for block in parse_json_blocks(&content, TRANSCRIPT_V1_MARKER) {
            if let Ok(record) = serde_json::from_str::<SessionTranscriptRecord>(&block) {
                if record.header.session_id == session_id
                    && invocation_id
                        .map(|expected| record.header.invocation_id == expected)
                        .unwrap_or(true)
                    && record.header.branch == branch
                {
                    records.push(record);
                }
            }
        }
    }

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

pub(crate) async fn remember_hot_messages(store: &MemoryStore, turn: &TalosTurn) {
    let mut cache = store.hot_cache.write().await;
    let entry = cache
        .entry(turn.session_id.clone())
        .or_insert_with(VecDeque::new);
    for message in &turn.messages {
        entry.push_back(message.clone());
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

    let session_dir = path_policy::session_dir(store, &turn.session_id);
    fs::create_dir_all(&session_dir)
        .await
        .map_err(|e| TalosError::StorageError(format!("create session memory dir: {}", e)))?;

    let day_file = session_dir.join(format!("{}.md", turn.timestamp.format("%Y-%m-%d")));
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(day_file)
        .await
        .map_err(|e| TalosError::StorageError(format!("open daily memory file: {}", e)))?;

    let payload = render_turn_block(turn)?;
    file.write_all(payload.as_bytes())
        .await
        .map_err(|e| TalosError::StorageError(format!("append daily memory file: {}", e)))?;

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

    let session_dir = path_policy::session_dir(store, &record.header.session_id);
    fs::create_dir_all(&session_dir)
        .await
        .map_err(|e| TalosError::StorageError(format!("create session memory dir: {}", e)))?;

    let day_file = session_dir.join(format!("{}.md", record.header.timestamp.format("%Y-%m-%d")));
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(day_file)
        .await
        .map_err(|e| TalosError::StorageError(format!("open daily memory file: {}", e)))?;

    let payload = render_transcript_block(record)?;
    file.write_all(payload.as_bytes())
        .await
        .map_err(|e| TalosError::StorageError(format!("append daily memory file: {}", e)))?;

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
                .flat_map(|turn| turn.messages)
                .collect::<Vec<_>>();
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
    })
}

pub(crate) async fn save_summary(store: &MemoryStore, record: &SummaryRecord) -> TalosResult<()> {
    if record.header.session_id.trim().is_empty() {
        return Err(TalosError::ValidationError(
            "summary session_id cannot be empty".to_string(),
        ));
    }

    let session_dir = path_policy::session_dir(store, &record.header.session_id);
    fs::create_dir_all(&session_dir)
        .await
        .map_err(|e| TalosError::StorageError(format!("create session summary dir: {}", e)))?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.summary_file(&record.header.session_id))
        .await
        .map_err(|e| TalosError::StorageError(format!("open summary file: {}", e)))?;

    let payload = render_summary_block(record)?;
    file.write_all(payload.as_bytes())
        .await
        .map_err(|e| TalosError::StorageError(format!("append summary file: {}", e)))?;
    Ok(())
}

pub(crate) async fn load_summaries(
    store: &MemoryStore,
    session_id: &str,
    limit: usize,
) -> TalosResult<Vec<SummaryRecord>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let summary_file = store.summary_file(session_id);
    let content = match fs::read_to_string(summary_file).await {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(TalosError::StorageError(format!(
                "read summary file: {}",
                err
            )));
        }
    };

    let mut records = Vec::new();
    for block in parse_json_blocks(&content, SUMMARY_V2_MARKER) {
        if let Ok(record) = serde_json::from_str::<SummaryRecord>(&block) {
            if record.header.session_id == session_id {
                records.push(record);
            }
        }
    }

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

    if let Ok(metadata) = fs::metadata(store.summary_file(session_id)).await {
        total = total.saturating_add(metadata.len());
    }

    Ok(total)
}
