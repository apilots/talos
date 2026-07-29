use serde::{Deserialize, Serialize};
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
        "medium" => Some(MemoryImportance::Normal),
        "normal" => Some(MemoryImportance::Normal),
        "high" => Some(MemoryImportance::High),
        _ => None,
    }
}

/// Store-owned representation of one indexed daily-memory block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryRecord {
    pub memory_id: String,
    pub title: String,
    pub body: String,
    pub kind: MemoryKind,
    pub importance: MemoryImportance,
    pub occurred_at: i64,
    pub source_date: String,
    pub session_id: Option<String>,
    pub source_from_seq: Option<u64>,
    pub source_until_seq: Option<u64>,
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemorySearchRecord {
    pub memory: MemoryRecord,
    pub preview: String,
    pub rank: u32,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedMemoryRecord {
    pub record: MemoryRecord,
    pub source_path: String,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedMemorySource {
    pub source_path: String,
    pub fingerprint: String,
    pub records: Vec<ParsedMemoryRecord>,
}
