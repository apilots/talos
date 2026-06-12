#[cfg(test)]
mod tests;
mod types;

pub use types::*;

use chrono::{DateTime, Utc};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use talos_config::{AgentKernelConfig, TalosTimezone};
use talos_llm_provider::LlmProvider;

use crate::path_policy;

#[derive(Clone)]
pub struct MemoryStore {
    pub(crate) workspace_dir: PathBuf,
    pub(crate) timezone: TalosTimezone,
    pub(crate) llm: Option<Arc<dyn LlmProvider>>,
    pub(crate) hot_recent_messages: usize,
    pub(crate) hot_cache: Arc<tokio::sync::RwLock<HashMap<String, VecDeque<TalosMessage>>>>,
    pub(crate) transcript_cache:
        Arc<tokio::sync::RwLock<HashMap<String, VecDeque<SessionTranscriptRecord>>>>,
    pub(crate) seq_cache: Arc<tokio::sync::RwLock<HashMap<String, u64>>>,
}

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryStore")
            .field("workspace_dir", &self.workspace_dir)
            .field("timezone", &self.timezone.to_string())
            .field("hot_recent_messages", &self.hot_recent_messages)
            .field("has_llm", &self.llm.is_some())
            .finish()
    }
}

impl MemoryStore {
    pub fn new(workspace_dir: impl Into<PathBuf>) -> Self {
        Self::new_internal(workspace_dir, chrono_tz::UTC, None, 64)
    }

    pub fn new_with_llm(workspace_dir: impl Into<PathBuf>, llm: Arc<dyn LlmProvider>) -> Self {
        Self::new_internal(workspace_dir, chrono_tz::UTC, Some(llm), 64)
    }

    fn new_internal(
        workspace_dir: impl Into<PathBuf>,
        timezone: TalosTimezone,
        llm: Option<Arc<dyn LlmProvider>>,
        hot_recent_messages: usize,
    ) -> Self {
        let workspace_dir = workspace_dir.into();
        Self {
            workspace_dir: match workspace_dir.to_str() {
                Some(path) => Self::resolve_workspace_dir(path),
                None => workspace_dir,
            },
            timezone,
            llm,
            hot_recent_messages,
            hot_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            transcript_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            seq_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    pub fn from_config(config: &AgentKernelConfig) -> Self {
        Self::new_internal(
            Self::resolve_workspace_dir(&effective_workspace_dir(config)),
            talos_config::parse_timezone(&config.timezone)
                .expect("agent kernel timezone should be validated before memory store init"),
            None,
            config.memory.hot_recent_messages.max(1),
        )
    }

    pub fn from_config_with_llm(config: &AgentKernelConfig, llm: Arc<dyn LlmProvider>) -> Self {
        Self::new_internal(
            Self::resolve_workspace_dir(&effective_workspace_dir(config)),
            talos_config::parse_timezone(&config.timezone)
                .expect("agent kernel timezone should be validated before memory store init"),
            Some(llm),
            config.memory.hot_recent_messages.max(1),
        )
    }

    pub fn new_default() -> Self {
        Self::new(Self::default_workspace_dir())
    }

    pub async fn load_session_turns(
        &self,
        session_id: &str,
    ) -> talos_error::TalosResult<Vec<TalosTurn>> {
        crate::session_store::load_turns(self, session_id).await
    }

    pub fn default_workspace_dir() -> PathBuf {
        path_policy::default_workspace_dir()
    }

    pub fn workspace_dir(&self) -> &Path {
        &self.workspace_dir
    }

    pub(crate) fn timezone(&self) -> TalosTimezone {
        self.timezone
    }

    pub(crate) fn resolve_workspace_dir(path: &str) -> PathBuf {
        path_policy::resolve_workspace_dir(path)
    }

    pub(crate) fn workspace_memory_dir(&self) -> PathBuf {
        path_policy::workspace_memory_dir(self)
    }

    pub(crate) fn workspace_memory_file_for(&self, timestamp: DateTime<Utc>) -> PathBuf {
        path_policy::workspace_memory_file_for(self, timestamp)
    }

    pub fn llm_provider(&self) -> Option<Arc<dyn LlmProvider>> {
        self.llm.clone()
    }
}

fn effective_workspace_dir(config: &AgentKernelConfig) -> String {
    let workspace_dir = config.workspace_dir.trim();
    workspace_dir.to_string()
}
