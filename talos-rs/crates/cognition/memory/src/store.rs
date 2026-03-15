use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use talos_config::AgentKernelMemoryConfig;
use talos_error::{TalosError, TalosResult};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;

use crate::Memory;

const DAY_FILE_PATTERN_LEN: usize = 10; // YYYY-MM-DD
const TURN_V2_MARKER: &str = "```json turn_v2";
const SUMMARY_V2_MARKER: &str = "```json summary_v2";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalMessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordHeader {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalMessage {
    pub header: RecordHeader,
    pub role: CanonicalMessageRole,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub call_id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultRecord {
    pub header: RecordHeader,
    pub call_id: String,
    pub tool: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub size_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalTurn {
    pub turn_id: String,
    pub session_id: String,
    pub seq: u64,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub messages: Vec<CanonicalMessage>,
    #[serde(default)]
    pub reasoning: Vec<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallRecord>,
    #[serde(default)]
    pub tool_results: Vec<ToolResultRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryRecord {
    pub version: u8,
    pub header: RecordHeader,
    pub covers_until_seq: u64,
    pub source_turn_count: usize,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryStorePromptContext {
    #[serde(default)]
    pub summaries: Vec<SummaryRecord>,
    #[serde(default)]
    pub recent_messages: Vec<CanonicalMessage>,
}

#[derive(Clone)]
pub struct MemoryStore {
    workspace_dir: PathBuf,
    hot_recent_messages: usize,
    hot_cache: Arc<tokio::sync::RwLock<HashMap<String, VecDeque<CanonicalMessage>>>>,
    seq_cache: Arc<tokio::sync::RwLock<HashMap<String, u64>>>,
}

impl MemoryStore {
    pub fn new(workspace_dir: impl Into<PathBuf>) -> Self {
        Self {
            workspace_dir: workspace_dir.into(),
            hot_recent_messages: 64,
            hot_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            seq_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    pub fn from_config(config: &AgentKernelMemoryConfig) -> Self {
        Self {
            workspace_dir: Self::resolve_workspace_dir(&config.workspace_dir),
            hot_recent_messages: config.hot_recent_messages.max(1),
            hot_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            seq_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    pub fn new_default() -> Self {
        Self::new(Self::default_workspace_dir())
    }

    pub fn default_workspace_dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".talos")
            .join("workspace")
            .join("memory")
    }

    pub fn workspace_dir(&self) -> &Path {
        &self.workspace_dir
    }

    fn resolve_workspace_dir(path: &str) -> PathBuf {
        if let Some(stripped) = path.strip_prefix("~/") {
            if let Ok(home) = std::env::var("HOME") {
                return PathBuf::from(home).join(stripped);
            }
        }
        PathBuf::from(path)
    }

    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.workspace_dir
            .join("sessions")
            .join(Self::sanitize_segment(session_id))
    }

    fn summary_file(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("summary.md")
    }

    async fn list_daily_files(&self, session_id: &str) -> TalosResult<Vec<PathBuf>> {
        let mut files = Vec::new();
        let session_dir = self.session_dir(session_id);

        let mut dir = match fs::read_dir(&session_dir).await {
            Ok(dir) => dir,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(files),
            Err(err) => {
                return Err(TalosError::StorageError(format!(
                    "read session memory dir: {}",
                    err
                )));
            }
        };

        while let Some(entry) = dir
            .next_entry()
            .await
            .map_err(|e| TalosError::StorageError(format!("read session memory entry: {}", e)))?
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !Self::is_daily_file_name(name) {
                continue;
            }
            files.push(path);
        }

        Ok(files)
    }

    fn is_daily_file_name(file_name: &str) -> bool {
        if !file_name.ends_with(".md") {
            return false;
        }

        let date = &file_name[..file_name.len() - 3];
        if date.len() != DAY_FILE_PATTERN_LEN {
            return false;
        }

        let chars: Vec<char> = date.chars().collect();
        for (idx, ch) in chars.iter().enumerate() {
            let is_dash = idx == 4 || idx == 7;
            if is_dash {
                if *ch != '-' {
                    return false;
                }
            } else if !ch.is_ascii_digit() {
                return false;
            }
        }

        true
    }

    fn sanitize_segment(input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        for ch in input.chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                out.push(ch);
            } else {
                out.push('_');
            }
        }
        if out.trim_matches('_').is_empty() {
            "session".to_string()
        } else {
            out
        }
    }

    fn render_turn_block(turn: &CanonicalTurn) -> TalosResult<String> {
        let payload = serde_json::to_string_pretty(turn).map_err(|e| {
            TalosError::SerializationError(format!("serialize canonical turn: {}", e))
        })?;
        Ok(format!(
            "## Turn {}\n{}\n{}\n```\n\n",
            turn.turn_id, TURN_V2_MARKER, payload
        ))
    }

    fn render_summary_block(record: &SummaryRecord) -> TalosResult<String> {
        let payload = serde_json::to_string_pretty(record).map_err(|e| {
            TalosError::SerializationError(format!("serialize summary record: {}", e))
        })?;
        Ok(format!(
            "## Summary {}\n{}\n{}\n```\n\n",
            record.header.id, SUMMARY_V2_MARKER, payload
        ))
    }

    fn parse_json_blocks(content: &str, marker: &str) -> Vec<String> {
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

    async fn load_turns(&self, session_id: &str) -> TalosResult<Vec<CanonicalTurn>> {
        let mut files = self.list_daily_files(session_id).await?;
        files.sort();

        let mut turns = Vec::new();
        for path in files {
            let content = fs::read_to_string(path)
                .await
                .map_err(|e| TalosError::StorageError(format!("read daily turn file: {}", e)))?;
            for block in Self::parse_json_blocks(&content, TURN_V2_MARKER) {
                if let Ok(turn) = serde_json::from_str::<CanonicalTurn>(&block) {
                    if turn.session_id == session_id {
                        turns.push(turn);
                    }
                }
            }
        }

        turns.sort_by(|a, b| a.seq.cmp(&b.seq).then(a.timestamp.cmp(&b.timestamp)));
        Ok(turns)
    }

    fn dedupe_keep_latest(messages: Vec<CanonicalMessage>) -> Vec<CanonicalMessage> {
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

    async fn remember_hot_messages(&self, turn: &CanonicalTurn) {
        let mut cache = self.hot_cache.write().await;
        let entry = cache
            .entry(turn.session_id.clone())
            .or_insert_with(VecDeque::new);
        for message in &turn.messages {
            entry.push_back(message.clone());
        }
        while entry.len() > self.hot_recent_messages {
            entry.pop_front();
        }
    }
}

#[async_trait]
impl Memory for MemoryStore {
    async fn next_turn_seq(&self, session_id: &str) -> TalosResult<u64> {
        {
            let mut seq_cache = self.seq_cache.write().await;
            if let Some(next) = seq_cache.get_mut(session_id) {
                let seq = *next;
                *next = next.saturating_add(1);
                return Ok(seq);
            }
        }

        let max_seq = self.latest_seq(session_id).await?;
        let seq = max_seq.saturating_add(1);
        let mut seq_cache = self.seq_cache.write().await;
        seq_cache.insert(session_id.to_string(), seq.saturating_add(1));
        Ok(seq)
    }

    async fn append_turn(&self, turn: &CanonicalTurn) -> TalosResult<()> {
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

        let session_dir = self.session_dir(&turn.session_id);
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

        let payload = Self::render_turn_block(turn)?;
        file.write_all(payload.as_bytes())
            .await
            .map_err(|e| TalosError::StorageError(format!("append daily memory file: {}", e)))?;

        self.remember_hot_messages(turn).await;

        {
            let mut seq_cache = self.seq_cache.write().await;
            let next = turn.seq.saturating_add(1);
            let entry = seq_cache.entry(turn.session_id.clone()).or_insert(next);
            if *entry < next {
                *entry = next;
            }
        }

        Ok(())
    }

    async fn build_prompt_context(
        &self,
        session_id: &str,
        target_recent_messages: usize,
        include_summary: bool,
        summary_recent_count: usize,
        summary_max_chars: usize,
    ) -> TalosResult<MemoryStorePromptContext> {
        let recent_messages = if target_recent_messages == 0 {
            Vec::new()
        } else {
            let hot = {
                let cache = self.hot_cache.read().await;
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
                let turns = self.load_turns(session_id).await?;
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
                let deduped = Self::dedupe_keep_latest(from_disk);
                if deduped.len() > target_recent_messages {
                    deduped[deduped.len() - target_recent_messages..].to_vec()
                } else {
                    deduped
                }
            }
        };

        let summaries = if include_summary && summary_recent_count > 0 {
            let mut records = self
                .load_summaries(session_id, summary_recent_count)
                .await?;
            if summary_max_chars > 0 {
                for record in &mut records {
                    record.content = Self::truncate_chars(&record.content, summary_max_chars);
                }
            }
            records
        } else {
            Vec::new()
        };

        Ok(MemoryStorePromptContext {
            summaries,
            recent_messages,
        })
    }

    async fn save_summary(&self, record: &SummaryRecord) -> TalosResult<()> {
        if record.header.session_id.trim().is_empty() {
            return Err(TalosError::ValidationError(
                "summary session_id cannot be empty".to_string(),
            ));
        }

        let session_dir = self.session_dir(&record.header.session_id);
        fs::create_dir_all(&session_dir)
            .await
            .map_err(|e| TalosError::StorageError(format!("create session summary dir: {}", e)))?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.summary_file(&record.header.session_id))
            .await
            .map_err(|e| TalosError::StorageError(format!("open summary file: {}", e)))?;

        let payload = Self::render_summary_block(record)?;
        file.write_all(payload.as_bytes())
            .await
            .map_err(|e| TalosError::StorageError(format!("append summary file: {}", e)))?;
        Ok(())
    }

    async fn load_summaries(
        &self,
        session_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<SummaryRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let summary_file = self.summary_file(session_id);
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
        for block in Self::parse_json_blocks(&content, SUMMARY_V2_MARKER) {
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

    async fn latest_seq(&self, session_id: &str) -> TalosResult<u64> {
        {
            let seq_cache = self.seq_cache.read().await;
            if let Some(next_seq) = seq_cache.get(session_id) {
                return Ok(next_seq.saturating_sub(1));
            }
        }

        let turns = self.load_turns(session_id).await?;
        let max_seq = turns.last().map(|turn| turn.seq).unwrap_or(0);
        Ok(max_seq)
    }

    async fn session_size_bytes(&self, session_id: &str) -> TalosResult<u64> {
        let mut total = 0_u64;
        for path in self.list_daily_files(session_id).await? {
            let metadata = fs::metadata(path)
                .await
                .map_err(|e| TalosError::StorageError(format!("read daily metadata: {}", e)))?;
            total = total.saturating_add(metadata.len());
        }

        if let Ok(metadata) = fs::metadata(self.summary_file(session_id)).await {
            total = total.saturating_add(metadata.len());
        }

        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_temp_store() -> (tempfile::TempDir, MemoryStore) {
        let dir = tempfile::tempdir().expect("tempdir should be created");
        let store = MemoryStore::new(dir.path().to_path_buf());
        (dir, store)
    }

    #[tokio::test]
    async fn test_turn_roundtrip_and_context_dedup() {
        let (_guard, store) = new_temp_store();
        let session_id = "session-1";

        let seq1 = store
            .next_turn_seq(session_id)
            .await
            .expect("seq should be 1");
        assert_eq!(seq1, 1);

        let turn_1 = CanonicalTurn {
            turn_id: "t1".to_string(),
            session_id: session_id.to_string(),
            seq: seq1,
            timestamp: Utc::now(),
            messages: vec![
                CanonicalMessage {
                    header: RecordHeader {
                        id: "t1:u".to_string(),
                        session_id: session_id.to_string(),
                        turn_id: Some("t1".to_string()),
                        timestamp: Utc::now(),
                        metadata: HashMap::new(),
                    },
                    role: CanonicalMessageRole::User,
                    text: "hello".to_string(),
                },
                CanonicalMessage {
                    header: RecordHeader {
                        id: "t1:a".to_string(),
                        session_id: session_id.to_string(),
                        turn_id: Some("t1".to_string()),
                        timestamp: Utc::now(),
                        metadata: HashMap::new(),
                    },
                    role: CanonicalMessageRole::Assistant,
                    text: "world".to_string(),
                },
            ],
            reasoning: Vec::new(),
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
        };

        store
            .append_turn(&turn_1)
            .await
            .expect("append should succeed");

        let seq2 = store
            .next_turn_seq(session_id)
            .await
            .expect("seq should be 2");
        assert_eq!(seq2, 2);

        let context = store
            .build_prompt_context(session_id, 8, false, 0, 0)
            .await
            .expect("context should build");
        assert_eq!(context.recent_messages.len(), 2);
    }

    #[tokio::test]
    async fn test_summary_records() {
        let (_guard, store) = new_temp_store();
        let record = SummaryRecord {
            version: 2,
            header: RecordHeader {
                id: "sum-1".to_string(),
                session_id: "s1".to_string(),
                turn_id: None,
                timestamp: Utc::now(),
                metadata: HashMap::new(),
            },
            covers_until_seq: 9,
            source_turn_count: 12,
            content: "facts".to_string(),
        };

        store
            .save_summary(&record)
            .await
            .expect("save summary should succeed");
        let loaded = store
            .load_summaries("s1", 4)
            .await
            .expect("load summary should succeed");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].header.id, "sum-1");
    }

    #[tokio::test]
    async fn test_build_prompt_context_includes_summary() {
        let (_guard, store) = new_temp_store();
        let session_id = "s1";
        let timestamp = Utc::now();

        store
            .append_turn(&CanonicalTurn {
                turn_id: "turn-1".to_string(),
                session_id: session_id.to_string(),
                seq: store
                    .next_turn_seq(session_id)
                    .await
                    .expect("seq should load"),
                timestamp,
                messages: vec![CanonicalMessage {
                    header: RecordHeader {
                        id: "turn-1:u".to_string(),
                        session_id: session_id.to_string(),
                        turn_id: Some("turn-1".to_string()),
                        timestamp,
                        metadata: HashMap::new(),
                    },
                    role: CanonicalMessageRole::User,
                    text: "remember this".to_string(),
                }],
                reasoning: Vec::new(),
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
            })
            .await
            .expect("append turn should succeed");

        store
            .save_summary(&SummaryRecord {
                version: 2,
                header: RecordHeader {
                    id: "summary-1".to_string(),
                    session_id: session_id.to_string(),
                    turn_id: None,
                    timestamp,
                    metadata: HashMap::new(),
                },
                covers_until_seq: 1,
                source_turn_count: 1,
                content: "facts".to_string(),
            })
            .await
            .expect("save summary should succeed");

        let context = store
            .build_prompt_context(session_id, 10, true, 4, 0)
            .await
            .expect("context should load");
        assert_eq!(context.recent_messages.len(), 1);
        assert_eq!(context.recent_messages[0].text, "remember this");
        assert_eq!(context.summaries.len(), 1);
        assert_eq!(context.summaries[0].header.id, "summary-1");
    }

    #[tokio::test]
    async fn test_session_size_bytes_after_append_and_summary() {
        let (_guard, store) = new_temp_store();
        let session_id = "s-size";
        let timestamp = Utc::now();

        store
            .append_turn(&CanonicalTurn {
                turn_id: "turn-size".to_string(),
                session_id: session_id.to_string(),
                seq: store
                    .next_turn_seq(session_id)
                    .await
                    .expect("seq should load"),
                timestamp,
                messages: vec![CanonicalMessage {
                    header: RecordHeader {
                        id: "turn-size:u".to_string(),
                        session_id: session_id.to_string(),
                        turn_id: Some("turn-size".to_string()),
                        timestamp,
                        metadata: HashMap::new(),
                    },
                    role: CanonicalMessageRole::User,
                    text: "payload".to_string(),
                }],
                reasoning: Vec::new(),
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
            })
            .await
            .expect("append turn should succeed");

        store
            .save_summary(&SummaryRecord {
                version: 2,
                header: RecordHeader {
                    id: "summary-size".to_string(),
                    session_id: session_id.to_string(),
                    turn_id: None,
                    timestamp,
                    metadata: HashMap::new(),
                },
                covers_until_seq: 1,
                source_turn_count: 1,
                content: "summary".to_string(),
            })
            .await
            .expect("save summary should succeed");

        let size = store
            .session_size_bytes(session_id)
            .await
            .expect("session size should load");
        assert!(size > 0);
    }

    #[tokio::test]
    async fn test_latest_seq_tracks_appended_turns() {
        let (_guard, store) = new_temp_store();
        let session_id = "s-seq";
        let timestamp = Utc::now();

        for idx in 1..=2 {
            let turn_id = format!("turn-{idx}");
            store
                .append_turn(&CanonicalTurn {
                    turn_id: turn_id.clone(),
                    session_id: session_id.to_string(),
                    seq: store
                        .next_turn_seq(session_id)
                        .await
                        .expect("seq should load"),
                    timestamp,
                    messages: vec![CanonicalMessage {
                        header: RecordHeader {
                            id: format!("{turn_id}:u"),
                            session_id: session_id.to_string(),
                            turn_id: Some(turn_id.clone()),
                            timestamp,
                            metadata: HashMap::new(),
                        },
                        role: CanonicalMessageRole::User,
                        text: format!("message {idx}"),
                    }],
                    reasoning: Vec::new(),
                    tool_calls: Vec::new(),
                    tool_results: Vec::new(),
                })
                .await
                .expect("append turn should succeed");
        }

        assert_eq!(
            store
                .latest_seq(session_id)
                .await
                .expect("latest seq should load"),
            2
        );
    }
}
