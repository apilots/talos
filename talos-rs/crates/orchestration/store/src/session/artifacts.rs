use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::{ChatMessage, ReasoningContinuation};
use talos_types::talos::{DynamicToolArguments, JsonContentValue, RunItem, StopReason, TokenUsage};

#[path = "artifact_reader.rs"]
mod artifact_reader;

use artifact_reader::{load_recent_artifacts, load_turn_window, synthesized_cancelled_turns};

const DAILY_FILE_NAME_LEN: usize = 16;
const SEQUENCE_STATE_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordHeader {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TalosTurn {
    pub turn_id: String,
    #[serde(default)]
    pub message_id: String,
    pub session_id: String,
    pub seq: u64,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub items: Vec<RunItem>,
    #[serde(default)]
    pub usage: TokenUsage,
    #[serde(default)]
    pub stop_reason: StopReason,
}

/// Selects which bounded sequence window a Session artifact caller needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTurnWindow {
    Latest,
    OldestAfter { seq: u64 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionTranscriptBranch {
    DirectAnswer,
    ReactiveLoop,
    ModelThinking,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionTranscriptAuthor {
    User,
    Agent,
    Tool,
    System,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionTranscriptEventKind {
    UserMessage,
    AgentMessage,
    ToolCall,
    ToolResult,
    SystemMessage,
    Planning,
    StateUpdate,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionTranscriptStateNamespace {
    App,
    User,
    Temp,
    Session,
    Workspace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionTranscriptStateDelta {
    pub namespace: SessionTranscriptStateNamespace,
    pub key: String,
    #[serde(default)]
    pub value: JsonContentValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SessionTranscriptActions {
    #[serde(default)]
    pub state_delta: Vec<SessionTranscriptStateDelta>,
    #[serde(default)]
    pub end_turn: bool,
    pub transfer_to: Option<String>,
    pub branch_override: Option<SessionTranscriptBranch>,
    pub rewind_before_invocation_id: Option<String>,
    pub compaction: Option<SessionTranscriptCompaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTranscriptCompaction {
    pub start_event_id: String,
    pub end_event_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTranscriptHeader {
    pub id: String,
    pub session_id: String,
    pub invocation_id: String,
    pub branch: SessionTranscriptBranch,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionTranscriptPayload {
    Message {
        message: ChatMessage,
    },
    StateOnly,
    Transfer {
        payload: SessionTranscriptTransferPayload,
    },
    Auth {
        payload: SessionTranscriptAuthPayload,
    },
    UserActionRequest {
        payload: SessionTranscriptUserActionRequestPayload,
    },
    UserActionResponse {
        payload: SessionTranscriptUserActionResponsePayload,
    },
    Compaction {
        payload: SessionTranscriptCompactionPayload,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTranscriptTransferPayload {
    pub target_agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTranscriptAuthPayload {
    pub provider: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionTranscriptUserActionRequestPayload {
    pub action_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub arguments: DynamicToolArguments,
    pub params: talos_types::UserActionParams,
    pub sandbox_request_fingerprint: Option<String>,
    pub reasoning_continuation: Option<ReasoningContinuation>,
    pub tool_calls: Vec<talos_llm_provider::FunctionCall>,
    pub tool_messages: Vec<ChatMessage>,
    pub deferred_tool_calls: Vec<talos_llm_provider::FunctionCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionTranscriptUserActionResponsePayload {
    pub call_id: String,
    pub tool_name: String,
    pub response: talos_types::UserActionResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionTranscriptCompactionPayload {
    pub summary_message: ChatMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionTranscriptRecord {
    pub header: SessionTranscriptHeader,
    pub author: SessionTranscriptAuthor,
    pub kind: SessionTranscriptEventKind,
    pub payload: SessionTranscriptPayload,
    #[serde(default)]
    pub actions: SessionTranscriptActions,
    #[serde(default)]
    pub partial: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SummaryRecord {
    pub version: u8,
    pub header: RecordHeader,
    pub covers_until_seq: u64,
    pub source_turn_count: usize,
    pub content: String,
}

/// Persists recoverable session artifacts below a caller-resolved workspace root.
///
/// Callers own workspace and user routing. Implementations own the artifact file format,
/// ordering, validation, and sequence allocation, but must not build prompts or long-term memory.
#[async_trait]
pub trait SessionArtifactStore: Send + Sync {
    async fn next_turn_seq(&self, workspace_root: &Path, session_id: &str) -> TalosResult<u64>;
    async fn append_turn(&self, workspace_root: &Path, turn: &TalosTurn) -> TalosResult<()>;
    async fn load_turns(
        &self,
        workspace_root: &Path,
        session_id: &str,
    ) -> TalosResult<Vec<TalosTurn>>;
    /// Loads only the ordered Turn window needed by prompt or maintenance callers.
    async fn load_turn_window(
        &self,
        workspace_root: &Path,
        session_id: &str,
        window: SessionTurnWindow,
        limit: usize,
    ) -> TalosResult<Vec<TalosTurn>>;
    async fn append_transcript_record(
        &self,
        workspace_root: &Path,
        record: &SessionTranscriptRecord,
    ) -> TalosResult<()>;
    async fn load_transcript_records(
        &self,
        workspace_root: &Path,
        session_id: &str,
        invocation_id: Option<&str>,
        branch: Option<SessionTranscriptBranch>,
    ) -> TalosResult<Vec<SessionTranscriptRecord>>;
    async fn save_summary(&self, workspace_root: &Path, record: &SummaryRecord) -> TalosResult<()>;
    async fn load_summaries(
        &self,
        workspace_root: &Path,
        session_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<SummaryRecord>>;
    async fn latest_seq(&self, workspace_root: &Path, session_id: &str) -> TalosResult<u64>;
    async fn session_size_bytes(&self, workspace_root: &Path, session_id: &str)
        -> TalosResult<u64>;
    async fn reset_session(&self, workspace_root: &Path, session_id: &str) -> TalosResult<()>;
}

#[derive(Clone, Default)]
pub struct FileSessionArtifactStore {
    seq_cache: Arc<tokio::sync::RwLock<HashMap<(PathBuf, String), u64>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SequenceState {
    version: u8,
    next_turn_seq: u64,
}

impl std::fmt::Debug for FileSessionArtifactStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("FileSessionArtifactStore").finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SessionArtifactRecord {
    Turn {
        turn: TalosTurn,
    },
    Transcript {
        record: Box<SessionTranscriptRecord>,
    },
    Summary {
        record: SummaryRecord,
    },
}

impl SessionArtifactRecord {
    fn timestamp(&self) -> DateTime<Utc> {
        match self {
            Self::Turn { turn } => turn.timestamp,
            Self::Transcript { record } => record.header.timestamp,
            Self::Summary { record } => record.header.timestamp,
        }
    }

    fn session_id(&self) -> &str {
        match self {
            Self::Turn { turn } => &turn.session_id,
            Self::Transcript { record } => &record.header.session_id,
            Self::Summary { record } => &record.header.session_id,
        }
    }
}

fn sanitize_segment(input: &str) -> String {
    let value = input
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if value.trim_matches('_').is_empty() {
        "session".to_string()
    } else {
        value
    }
}

fn session_dir(workspace_root: &Path, session_id: &str) -> PathBuf {
    workspace_root
        .join("sessions")
        .join(sanitize_segment(session_id))
}

fn day_file(workspace_root: &Path, session_id: &str, timestamp: DateTime<Utc>) -> PathBuf {
    session_dir(workspace_root, session_id).join(format!("{}.jsonl", timestamp.format("%Y-%m-%d")))
}

fn sequence_state_file(workspace_root: &Path, session_id: &str) -> PathBuf {
    session_dir(workspace_root, session_id).join("sequence.json")
}

async fn load_sequence_state(
    workspace_root: &Path,
    session_id: &str,
) -> TalosResult<Option<SequenceState>> {
    let path = sequence_state_file(workspace_root, session_id);
    let Some(content) = talos_fs::read_file(&path).await? else {
        return Ok(None);
    };
    let state = serde_json::from_str::<SequenceState>(&content).map_err(|error| {
        TalosError::SerializationError(format!(
            "parse session sequence state {}: {error}",
            path.display()
        ))
    })?;
    if state.version != SEQUENCE_STATE_VERSION || state.next_turn_seq == 0 {
        return Err(TalosError::ValidationError(format!(
            "invalid session sequence state: {}",
            path.display()
        )));
    }
    Ok(Some(state))
}

async fn save_sequence_state(
    workspace_root: &Path,
    session_id: &str,
    next_turn_seq: u64,
) -> TalosResult<()> {
    let state = SequenceState {
        version: SEQUENCE_STATE_VERSION,
        next_turn_seq,
    };
    let content = serde_json::to_string(&state).map_err(|error| {
        TalosError::SerializationError(format!("serialize session sequence state: {error}"))
    })?;
    talos_fs::write_file(&sequence_state_file(workspace_root, session_id), &content).await
}

fn is_day_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    name.len() == DAILY_FILE_NAME_LEN
        && name.ends_with(".jsonl")
        && name.get(..10).is_some_and(|date| {
            date.bytes().enumerate().all(|(index, byte)| {
                if matches!(index, 4 | 7) {
                    byte == b'-'
                } else {
                    byte.is_ascii_digit()
                }
            })
        })
}

async fn list_day_files(workspace_root: &Path, session_id: &str) -> TalosResult<Vec<PathBuf>> {
    let Some(entries) = talos_fs::read_directory(&session_dir(workspace_root, session_id))? else {
        return Ok(Vec::new());
    };
    let mut files = entries
        .into_iter()
        .filter(|entry| entry.metadata.is_file && is_day_file(&entry.path))
        .map(|entry| entry.path)
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

async fn append_artifact(
    workspace_root: &Path,
    session_id: &str,
    timestamp: DateTime<Utc>,
    record: &SessionArtifactRecord,
) -> TalosResult<()> {
    let line = serde_json::to_string(record).map_err(|error| {
        TalosError::SerializationError(format!("serialize session artifact: {error}"))
    })?;
    talos_fs::append_file(
        &day_file(workspace_root, session_id, timestamp),
        &format!("{line}\n"),
    )
    .await
}

async fn load_artifacts(
    workspace_root: &Path,
    session_id: &str,
) -> TalosResult<Vec<SessionArtifactRecord>> {
    let mut records = Vec::new();
    for path in list_day_files(workspace_root, session_id).await? {
        let content = talos_fs::read_file(&path).await?.ok_or_else(|| {
            TalosError::StorageError(format!("session artifact file missing: {}", path.display()))
        })?;
        for (line_index, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            records.push(parse_artifact_line(&path, line_index, line, session_id)?);
        }
    }
    records.sort_by_key(SessionArtifactRecord::timestamp);
    Ok(records)
}

fn parse_artifact_line(
    path: &Path,
    line_index: usize,
    line: &str,
    session_id: &str,
) -> TalosResult<SessionArtifactRecord> {
    parse_artifact_record(
        path,
        &format!("line {}", line_index.saturating_add(1)),
        line,
        session_id,
    )
}

fn parse_artifact_record(
    path: &Path,
    location: &str,
    line: &str,
    session_id: &str,
) -> TalosResult<SessionArtifactRecord> {
    let record: SessionArtifactRecord = serde_json::from_str(line).map_err(|error| {
        TalosError::SerializationError(format!(
            "parse session artifact {} at {location}: {error}",
            path.display(),
        ))
    })?;
    if record.session_id() != session_id {
        return Err(TalosError::ValidationError(format!(
            "session artifact {} at {location} belongs to session {}, expected {session_id}",
            path.display(),
            record.session_id()
        )));
    }
    Ok(record)
}

#[async_trait]
impl SessionArtifactStore for FileSessionArtifactStore {
    async fn next_turn_seq(&self, workspace_root: &Path, session_id: &str) -> TalosResult<u64> {
        let key = (workspace_root.to_path_buf(), session_id.to_string());
        let mut cache = self.seq_cache.write().await;
        if let Some(next) = cache.get(&key) {
            return Ok(*next);
        }
        let persisted_next = load_sequence_state(workspace_root, session_id)
            .await?
            .map(|state| state.next_turn_seq)
            .unwrap_or(1);
        let artifact_next = self
            .load_turns(workspace_root, session_id)
            .await?
            .last()
            .map(|turn| turn.seq.saturating_add(1))
            .unwrap_or(1);
        let next = persisted_next.max(artifact_next);
        cache.insert(key, next);
        Ok(next)
    }

    async fn append_turn(&self, workspace_root: &Path, turn: &TalosTurn) -> TalosResult<()> {
        if turn.turn_id.trim().is_empty()
            || turn.message_id.trim().is_empty()
            || turn.session_id.trim().is_empty()
        {
            return Err(TalosError::ValidationError(
                "turn_id, message_id, and session_id are required".to_string(),
            ));
        }
        append_artifact(
            workspace_root,
            &turn.session_id,
            turn.timestamp,
            &SessionArtifactRecord::Turn { turn: turn.clone() },
        )
        .await?;
        let key = (workspace_root.to_path_buf(), turn.session_id.clone());
        let next = turn.seq.saturating_add(1);
        let mut cache = self.seq_cache.write().await;
        let next = cache.get(&key).copied().unwrap_or(1).max(next);
        cache.insert(key, next);
        if let Err(error) = save_sequence_state(workspace_root, &turn.session_id, next).await {
            tracing::warn!(
                session_id = %turn.session_id,
                turn_seq = turn.seq,
                next_turn_seq = next,
                path = %sequence_state_file(workspace_root, &turn.session_id).display(),
                error = %error,
                "failed to persist derived session sequence state after artifact append"
            );
        }
        Ok(())
    }

    async fn load_turns(
        &self,
        workspace_root: &Path,
        session_id: &str,
    ) -> TalosResult<Vec<TalosTurn>> {
        let artifacts = load_artifacts(workspace_root, session_id).await?;
        let mut turns = artifacts
            .iter()
            .filter_map(|artifact| match artifact {
                SessionArtifactRecord::Turn { turn } => Some(turn.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        turns.extend(synthesized_cancelled_turns(session_id, &artifacts, &turns));
        turns.sort_by(|left, right| {
            left.seq
                .cmp(&right.seq)
                .then(left.timestamp.cmp(&right.timestamp))
        });
        Ok(turns)
    }

    async fn load_turn_window(
        &self,
        workspace_root: &Path,
        session_id: &str,
        window: SessionTurnWindow,
        limit: usize,
    ) -> TalosResult<Vec<TalosTurn>> {
        load_turn_window(workspace_root, session_id, window, limit).await
    }

    async fn append_transcript_record(
        &self,
        workspace_root: &Path,
        record: &SessionTranscriptRecord,
    ) -> TalosResult<()> {
        if record.header.id.trim().is_empty()
            || record.header.session_id.trim().is_empty()
            || record.header.invocation_id.trim().is_empty()
        {
            return Err(TalosError::ValidationError(
                "transcript id, session_id, and invocation_id are required".to_string(),
            ));
        }
        append_artifact(
            workspace_root,
            &record.header.session_id,
            record.header.timestamp,
            &SessionArtifactRecord::Transcript {
                record: Box::new(record.clone()),
            },
        )
        .await
    }

    async fn load_transcript_records(
        &self,
        workspace_root: &Path,
        session_id: &str,
        invocation_id: Option<&str>,
        branch: Option<SessionTranscriptBranch>,
    ) -> TalosResult<Vec<SessionTranscriptRecord>> {
        let mut records = load_artifacts(workspace_root, session_id)
            .await?
            .into_iter()
            .filter_map(|artifact| match artifact {
                SessionArtifactRecord::Transcript { record }
                    if invocation_id.is_none_or(|value| record.header.invocation_id == value)
                        && branch.is_none_or(|value| record.header.branch == value) =>
                {
                    Some(*record)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        records.sort_by_key(|record| record.header.timestamp);
        Ok(records)
    }

    async fn save_summary(&self, workspace_root: &Path, record: &SummaryRecord) -> TalosResult<()> {
        if record.header.session_id.trim().is_empty() {
            return Err(TalosError::ValidationError(
                "summary session_id is required".to_string(),
            ));
        }
        append_artifact(
            workspace_root,
            &record.header.session_id,
            record.header.timestamp,
            &SessionArtifactRecord::Summary {
                record: record.clone(),
            },
        )
        .await
    }

    async fn load_summaries(
        &self,
        workspace_root: &Path,
        session_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<SummaryRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut records = Vec::with_capacity(limit);
        let mut files = list_day_files(workspace_root, session_id).await?;
        files.reverse();
        for path in files {
            let recent =
                load_recent_artifacts(&path, session_id, limit - records.len(), |record| {
                    matches!(record, SessionArtifactRecord::Summary { .. })
                })
                .await?;
            for artifact in recent {
                if let SessionArtifactRecord::Summary { record } = artifact {
                    records.push(record);
                }
            }
            if records.len() == limit {
                break;
            }
        }
        records.sort_by_key(|record| record.header.timestamp);
        Ok(records)
    }

    async fn latest_seq(&self, workspace_root: &Path, session_id: &str) -> TalosResult<u64> {
        let key = (workspace_root.to_path_buf(), session_id.to_string());
        if let Some(next) = self.seq_cache.read().await.get(&key) {
            return Ok(next.saturating_sub(1));
        }
        let artifact_latest = self
            .load_turn_window(workspace_root, session_id, SessionTurnWindow::Latest, 1)
            .await?
            .into_iter()
            .last()
            .map(|turn| turn.seq)
            .unwrap_or(0);
        let persisted_latest = load_sequence_state(workspace_root, session_id)
            .await?
            .map(|state| state.next_turn_seq.saturating_sub(1))
            .unwrap_or(0);
        Ok(artifact_latest.max(persisted_latest))
    }

    async fn session_size_bytes(
        &self,
        workspace_root: &Path,
        session_id: &str,
    ) -> TalosResult<u64> {
        let mut size = 0_u64;
        for path in list_day_files(workspace_root, session_id).await? {
            if let Some(metadata) = talos_fs::get_file_metadata(&path).await? {
                size = size.saturating_add(metadata.size_bytes);
            }
        }
        Ok(size)
    }

    async fn reset_session(&self, workspace_root: &Path, session_id: &str) -> TalosResult<()> {
        let key = (workspace_root.to_path_buf(), session_id.to_string());
        let mut cache = self.seq_cache.write().await;
        let persisted_next = load_sequence_state(workspace_root, session_id)
            .await?
            .map(|state| state.next_turn_seq)
            .unwrap_or(1);
        let artifact_next = self
            .load_turns(workspace_root, session_id)
            .await?
            .last()
            .map(|turn| turn.seq.saturating_add(1))
            .unwrap_or(1);
        let next = cache
            .get(&key)
            .copied()
            .unwrap_or(1)
            .max(persisted_next)
            .max(artifact_next);
        save_sequence_state(workspace_root, session_id, next).await?;
        for path in list_day_files(workspace_root, session_id).await? {
            talos_fs::remove_file(&path)?;
        }
        cache.insert(key, next);
        Ok(())
    }
}

#[cfg(test)]
#[path = "artifacts_tests.rs"]
mod tests;
