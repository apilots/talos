use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    #[default]
    Default,
    Plan,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReadFileStateEntry {
    pub path: PathBuf,
    pub observed_at_session_seq: u64,
    pub observed_at_turn: u64,
    pub content_hash: String,
    pub size_bytes: u64,
}

impl ReadFileStateEntry {
    pub fn from_content(path: PathBuf, content: &str, observed_at_session_seq: u64) -> Self {
        Self {
            path,
            observed_at_session_seq,
            observed_at_turn: observed_at_session_seq,
            content_hash: sha256_hex(content.as_bytes()),
            size_bytes: content.len() as u64,
        }
    }
}

#[derive(Debug, Default)]
pub struct ReadFileStateStore {
    seq: std::sync::atomic::AtomicU64,
    entries: DashMap<PathBuf, ReadFileStateEntry>,
}

impl ReadFileStateStore {
    pub fn record_content(&self, path: PathBuf, content: &str) -> ReadFileStateEntry {
        let sequence = self.seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let entry = ReadFileStateEntry::from_content(path.clone(), content, sequence);
        self.entries.insert(path, entry.clone());
        entry
    }

    pub fn get(&self, path: &Path) -> Option<ReadFileStateEntry> {
        self.entries.get(path).map(|entry| entry.clone())
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.entries.contains_key(path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskStateEntry {
    pub content: String,
}

#[derive(Debug, Default)]
pub struct TaskStateStore {
    entries: Arc<std::sync::RwLock<Vec<TaskStateEntry>>>,
}

impl TaskStateStore {
    pub fn replace(&self, entries: Vec<TaskStateEntry>) {
        if let Ok(mut guard) = self.entries.write() {
            *guard = entries;
        }
    }

    pub fn list(&self) -> Vec<TaskStateEntry> {
        self.entries
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

#[derive(Debug, Default)]
pub struct ToolDecisionState {
    decisions: DashMap<String, Value>,
}

impl ToolDecisionState {
    pub fn record(&self, key: impl Into<String>, value: Value) {
        self.decisions.insert(key.into(), value);
    }
}

#[derive(Debug, Default)]
pub struct DeferredToolState {
    activated: Arc<std::sync::RwLock<BTreeSet<String>>>,
}

impl DeferredToolState {
    pub fn activate(&self, tool_name: impl Into<String>) {
        if let Ok(mut guard) = self.activated.write() {
            guard.insert(tool_name.into());
        }
    }

    pub fn list(&self) -> Vec<String> {
        self.activated
            .read()
            .map(|guard| guard.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[derive(Debug, Default)]
pub struct ToolResultBudgetState {
    total_bytes: std::sync::atomic::AtomicUsize,
}

impl ToolResultBudgetState {
    pub fn add_bytes(&self, bytes: usize) {
        self.total_bytes
            .fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogToolEntry {
    pub public_name: String,
    pub description: String,
    pub deferred: bool,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Default)]
pub struct ToolCatalogState {
    entries: Arc<std::sync::RwLock<Vec<CatalogToolEntry>>>,
}

impl ToolCatalogState {
    pub fn replace(&self, entries: Vec<CatalogToolEntry>) {
        if let Ok(mut guard) = self.entries.write() {
            *guard = entries;
        }
    }

    pub fn list(&self) -> Vec<CatalogToolEntry> {
        self.entries
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

#[derive(Debug, Default)]
pub struct ContentReplacementState {
    replacements: DashMap<String, String>,
}

impl ContentReplacementState {
    pub fn record(&self, key: impl Into<String>, value: impl Into<String>) {
        self.replacements.insert(key.into(), value.into());
    }
}

#[derive(Debug, Default)]
pub struct ToolPermissionContext {
    mode: Arc<std::sync::RwLock<PermissionMode>>,
}

impl ToolPermissionContext {
    pub fn mode(&self) -> PermissionMode {
        self.mode.read().map(|guard| *guard).unwrap_or_default()
    }

    pub fn set_mode(&self, mode: PermissionMode) {
        if let Ok(mut guard) = self.mode.write() {
            *guard = mode;
        }
    }
}

#[derive(Debug, Default)]
pub struct SessionToolState {
    pub read_files: ReadFileStateStore,
    pub tasks: TaskStateStore,
    pub decisions: ToolDecisionState,
    pub deferred_tools: DeferredToolState,
    pub result_budget: ToolResultBudgetState,
    pub catalog: ToolCatalogState,
    pub content_replacement: ContentReplacementState,
    pub permissions: ToolPermissionContext,
}

pub fn get_session_tool_state(session_id: &str) -> Arc<SessionToolState> {
    static STATES: OnceLock<DashMap<String, Arc<SessionToolState>>> = OnceLock::new();
    STATES
        .get_or_init(DashMap::new)
        .entry(session_id.to_string())
        .or_insert_with(|| Arc::new(SessionToolState::default()))
        .clone()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
