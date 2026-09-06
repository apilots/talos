use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use talos_agent_kernel::{
    ExecutionBranch, KernelSessionArtifactStore, KernelSessionSummary, KernelTranscriptEvent,
};
use talos_error::{TalosError, TalosResult};

#[path = "artifact_reader.rs"]
mod artifact_reader;

use artifact_reader::load_recent_artifacts;

const DAILY_FILE_NAME_LEN: usize = 16;
const LEGACY_ARTIFACT_SCHEMA_VERSION: u8 = 1;
const ARTIFACT_SCHEMA_VERSION: u8 = 2;

#[derive(Clone, Default)]
pub struct FileKernelSessionArtifactStore;

impl std::fmt::Debug for FileKernelSessionArtifactStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FileKernelSessionArtifactStore")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SessionArtifactRecord {
    Transcript { record: Box<KernelTranscriptEvent> },
    Summary { record: KernelSessionSummary },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VersionedSessionArtifactRecord {
    schema_version: u8,
    #[serde(flatten)]
    record: SessionArtifactRecord,
}

impl SessionArtifactRecord {
    fn timestamp(&self) -> DateTime<Utc> {
        match self {
            Self::Transcript { record } => record.timestamp,
            Self::Summary { record } => record.header.timestamp,
        }
    }

    fn session_id(&self) -> &str {
        match self {
            Self::Transcript { record } => &record.session_id,
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
    let line = serde_json::to_string(&VersionedSessionArtifactRecord {
        schema_version: ARTIFACT_SCHEMA_VERSION,
        record: record.clone(),
    })
    .map_err(|error| TalosError::internal(format!("serialize session artifact: {error}")))?;
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
            TalosError::internal(format!("session artifact file missing: {}", path.display()))
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
    let value = serde_json::from_str(line).map_err(|error| {
        TalosError::internal(format!(
            "parse session artifact {} at {location}: {error}",
            path.display(),
        ))
    })?;
    let value = migrate_artifact_value(value).map_err(|error| {
        TalosError::internal(format!(
            "migrate session artifact {} at {location}: {error}",
            path.display(),
        ))
    })?;
    let versioned: VersionedSessionArtifactRecord =
        serde_json::from_value(value).map_err(|error| {
            TalosError::internal(format!(
                "parse session artifact {} at {location}: {error}",
                path.display(),
            ))
        })?;
    let record = versioned.record;
    if record.session_id() != session_id {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!(
                "session artifact {} at {location} belongs to session {}, expected {session_id}",
                path.display(),
                record.session_id()
            ),
        ));
    }
    Ok(record)
}

fn migrate_artifact_value(mut value: serde_json::Value) -> Result<serde_json::Value, String> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| "artifact record must be an object".to_string())?;
    let version = object
        .get("schema_version")
        .map(|value| {
            value
                .as_u64()
                .and_then(|value| u8::try_from(value).ok())
                .ok_or_else(|| "artifact schema_version must be an unsigned byte".to_string())
        })
        .transpose()?
        .unwrap_or(LEGACY_ARTIFACT_SCHEMA_VERSION);
    match version {
        ARTIFACT_SCHEMA_VERSION => return Ok(value),
        LEGACY_ARTIFACT_SCHEMA_VERSION => {}
        version => return Err(format!("unsupported artifact schema version {version}")),
    }

    if object.get("kind").and_then(serde_json::Value::as_str) == Some("transcript") {
        let record = object
            .get_mut("record")
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| "legacy transcript record must be an object".to_string())?;
        migrate_legacy_transcript(record)?;
    }
    object.insert(
        "schema_version".to_string(),
        serde_json::Value::from(ARTIFACT_SCHEMA_VERSION),
    );
    Ok(value)
}

fn migrate_legacy_transcript(
    record: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let state_only = record
        .get("payload")
        .and_then(serde_json::Value::as_object)
        .and_then(|payload| payload.get("type"))
        .and_then(serde_json::Value::as_str)
        == Some("state_only");
    let Some(actions) = record.remove("actions") else {
        if state_only {
            return Err("legacy state-only transcript is missing actions".to_string());
        }
        return Ok(());
    };
    let mut actions = actions
        .as_object()
        .cloned()
        .ok_or_else(|| "legacy transcript actions must be an object".to_string())?;
    let typed_update = actions
        .remove("session_state")
        .filter(|value| !value.is_null());
    let legacy_updates = actions
        .remove("state_delta")
        .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
    let mut updates = legacy_updates
        .as_array()
        .ok_or_else(|| "legacy state_delta must be an array".to_string())?
        .iter()
        .map(migrate_legacy_state_delta)
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(update) = typed_update {
        updates.push(update);
    }
    for (name, value) in actions {
        let is_default = match name.as_str() {
            "end_turn" => value == serde_json::Value::Bool(false),
            "transfer_to" | "branch_override" | "rewind_before_invocation_id" | "compaction" => {
                value.is_null()
            }
            _ => false,
        };
        if !is_default {
            return Err(format!("unsupported legacy transcript action `{name}`"));
        }
    }
    if updates.is_empty() {
        if state_only {
            return Err("legacy state-only transcript contains no Session state".to_string());
        }
        return Ok(());
    }
    if !state_only || updates.len() != 1 {
        return Err("legacy Session state must contain exactly one state-only update".to_string());
    }
    record.insert(
        "payload".to_string(),
        serde_json::json!({
            "type": "session_state",
            "update": updates.pop().expect("one legacy update exists"),
        }),
    );
    Ok(())
}

fn migrate_legacy_state_delta(value: &serde_json::Value) -> Result<serde_json::Value, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "legacy state delta must be an object".to_string())?;
    if object.get("namespace").and_then(serde_json::Value::as_str) != Some("session") {
        return Err("legacy state delta is not Session-owned".to_string());
    }
    let state_type = match object.get("key").and_then(serde_json::Value::as_str) {
        Some("prompt_warning_state") => "prompt_warning",
        Some("session_compaction_state") => "compaction",
        Some("session_continuity") => "continuity",
        Some(key) => return Err(format!("unsupported legacy Session state key `{key}`")),
        None => return Err("legacy state delta is missing its key".to_string()),
    };
    let data = object
        .get("value")
        .cloned()
        .ok_or_else(|| "legacy state delta is missing its value".to_string())?;
    Ok(serde_json::json!({ "type": state_type, "data": data }))
}

#[async_trait]
impl KernelSessionArtifactStore for FileKernelSessionArtifactStore {
    async fn append_transcript_event(
        &self,
        workspace_root: &Path,
        event: &KernelTranscriptEvent,
    ) -> TalosResult<()> {
        if event.id.trim().is_empty()
            || event.session_id.trim().is_empty()
            || event.invocation_id.trim().is_empty()
        {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "transcript id, session_id, and invocation_id are required".to_string(),
            ));
        }
        append_artifact(
            workspace_root,
            &event.session_id,
            event.timestamp,
            &SessionArtifactRecord::Transcript {
                record: Box::new(event.clone()),
            },
        )
        .await
    }

    async fn load_transcript_events(
        &self,
        workspace_root: &Path,
        session_id: &str,
        invocation_id: Option<&str>,
        branch: Option<ExecutionBranch>,
    ) -> TalosResult<Vec<KernelTranscriptEvent>> {
        let mut records = load_artifacts(workspace_root, session_id)
            .await?
            .into_iter()
            .filter_map(|artifact| match artifact {
                SessionArtifactRecord::Transcript { record }
                    if invocation_id.is_none_or(|value| record.invocation_id == value)
                        && branch.is_none_or(|value| record.branch == value) =>
                {
                    Some(*record)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        records.sort_by_key(|record| record.timestamp);
        Ok(records)
    }

    async fn save_summary(
        &self,
        workspace_root: &Path,
        record: &KernelSessionSummary,
    ) -> TalosResult<()> {
        if record.header.session_id.trim().is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
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
    ) -> TalosResult<Vec<KernelSessionSummary>> {
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
        for path in list_day_files(workspace_root, session_id).await? {
            talos_fs::remove_file(&path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "artifacts_tests.rs"]
mod tests;
