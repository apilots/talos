use chrono_tz::Tz;
use sha2::{Digest, Sha256};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_memory::{
    DreamMemoryDate, DreamMemoryDocument, DreamMemoryDocumentContext, DreamMemoryError,
    DreamMemoryErrorKind, MemoryAgentId, MemoryTimezone, MemoryUserId,
};

use super::types::{MemoryRecord, ParsedMemoryRecord, ParsedMemorySource};

pub(super) fn source_from_markdown(
    workspace_id: &str,
    user_id: &str,
    source_path: &str,
    content: &str,
    timezone: Tz,
) -> TalosResult<ParsedMemorySource> {
    let source_date = source_date(source_path)?;
    let context = DreamMemoryDocumentContext::new(
        MemoryAgentId::parse(workspace_id.to_string()).map_err(map_dream_memory_error)?,
        MemoryUserId::parse(user_id.to_string()).map_err(map_dream_memory_error)?,
        source_date,
        MemoryTimezone::new(timezone),
    );
    let document = DreamMemoryDocument::parse(context, content).map_err(map_dream_memory_error)?;
    let records = document
        .entries()
        .iter()
        .map(|entry| ParsedMemoryRecord {
            record: MemoryRecord {
                memory_id: entry.memory_id().clone(),
                title: entry.title().to_string(),
                body: entry.body().to_string(),
                kind: entry.kind(),
                importance: entry.importance(),
                occurred_at: entry.occurred_at(),
                source_date,
                provenance: entry.provenance().clone(),
            },
        })
        .collect();
    Ok(ParsedMemorySource {
        source_path: source_path.to_string(),
        source_date,
        fingerprint: sha256_hex(content.as_bytes()),
        records,
    })
}

pub(super) fn source_date(source_path: &str) -> TalosResult<DreamMemoryDate> {
    std::path::Path::new(source_path)
        .file_name()
        .and_then(|value| value.to_str())
        .and_then(|value| value.strip_suffix(".md"))
        .map(DreamMemoryDate::parse)
        .transpose()
        .map_err(map_dream_memory_error)?
        .ok_or_else(|| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "invalid Dream Memory source path".to_string(),
            )
        })
}

pub(super) fn map_dream_memory_error(error: DreamMemoryError) -> TalosError {
    let code = match error.kind() {
        DreamMemoryErrorKind::Conflict | DreamMemoryErrorKind::AlreadyExists => ErrorCode::Conflict,
        DreamMemoryErrorKind::NotFound => ErrorCode::ResourceNotFound,
        DreamMemoryErrorKind::InvalidInput
        | DreamMemoryErrorKind::InvalidDocument
        | DreamMemoryErrorKind::LimitExceeded => ErrorCode::InvalidRequest,
    };
    TalosError::new(code, error.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
