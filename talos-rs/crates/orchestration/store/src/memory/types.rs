use talos_error::{TalosError, TalosResult};
use talos_memory::{DreamMemoryDate, DreamMemoryProvenance, MemoryId, MemoryScope};
pub use talos_types::{MemoryImportance, MemoryKind};

pub(super) fn memory_kind_as_str(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Preference => "preference",
        MemoryKind::Fact => "fact",
        MemoryKind::Decision => "decision",
        MemoryKind::Task => "task",
        MemoryKind::Episode => "episode",
    }
}

pub(super) fn parse_memory_kind(value: &str) -> Option<MemoryKind> {
    match value {
        "preference" => Some(MemoryKind::Preference),
        "fact" => Some(MemoryKind::Fact),
        "decision" => Some(MemoryKind::Decision),
        "task" => Some(MemoryKind::Task),
        "episode" => Some(MemoryKind::Episode),
        _ => None,
    }
}

pub(super) fn memory_importance_as_str(importance: MemoryImportance) -> &'static str {
    match importance {
        MemoryImportance::Low => "low",
        MemoryImportance::Normal => "normal",
        MemoryImportance::High => "high",
    }
}

pub(super) fn parse_memory_importance(value: &str) -> Option<MemoryImportance> {
    match value {
        "low" => Some(MemoryImportance::Low),
        "normal" => Some(MemoryImportance::Normal),
        "high" => Some(MemoryImportance::High),
        _ => None,
    }
}

/// Store-owned typed representation of one indexed Dream Memory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRecord {
    pub memory_id: MemoryId,
    pub title: String,
    pub body: String,
    pub kind: MemoryKind,
    pub importance: MemoryImportance,
    pub occurred_at: i64,
    pub source_date: DreamMemoryDate,
    pub provenance: DreamMemoryProvenance,
}

/// Validated query consumed only by the memory store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySearchQuery {
    pub text: String,
    pub limit: u32,
    pub kinds: Option<Vec<MemoryKind>>,
    pub since_at: Option<i64>,
    pub until_at: Option<i64>,
}

/// Ranked store result converted to public DTOs by the service layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySearchRecord {
    pub memory: MemoryRecord,
    pub preview: String,
    pub rank: u32,
}

/// Validated identity used by every derived-index query and mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MemoryIndexScope {
    workspace_id: String,
    user_id: String,
    workspace_root: String,
}

impl MemoryIndexScope {
    pub(super) fn from_scope(scope: &MemoryScope) -> TalosResult<Self> {
        let workspace_id = scope.workspace_id.trim();
        let user_id = scope.user_id.trim();
        let workspace_root = scope.workspace_root.to_str().ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "memory workspace_root must be valid UTF-8".to_string(),
            )
        })?;
        if workspace_id.is_empty() || user_id.is_empty() || workspace_root.trim().is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "memory workspace_id, user_id, and workspace_root are required".to_string(),
            ));
        }
        Ok(Self {
            workspace_id: workspace_id.to_string(),
            user_id: user_id.to_string(),
            workspace_root: workspace_root.to_string(),
        })
    }

    pub(super) fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub(super) fn user_id(&self) -> &str {
        &self.user_id
    }

    pub(super) fn workspace_root(&self) -> &str {
        &self.workspace_root
    }
}

#[derive(Debug, Clone)]
pub(super) struct ParsedMemoryRecord {
    pub record: MemoryRecord,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedMemorySource {
    pub source_path: String,
    pub source_date: DreamMemoryDate,
    pub fingerprint: String,
    pub records: Vec<ParsedMemoryRecord>,
}
