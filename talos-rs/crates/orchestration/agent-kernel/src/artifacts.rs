use std::path::Path;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use talos_error::TalosResult;

use crate::{ExecutionBranch, KernelTranscriptEvent};

/// Metadata stored with one Provider-private Session summary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KernelArtifactHeader {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Provider-private summary used to rebuild Kernel prompt context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KernelSessionSummary {
    pub version: u8,
    pub header: KernelArtifactHeader,
    pub covers_until_seq: u64,
    pub source_turn_count: usize,
    pub content: String,
}

/// Persists Kernel-owned transcript and summary state below a resolved workspace root.
///
/// Implementations own file/database mechanics only. Product Session, Timeline,
/// History, and recovery facts must not cross this Provider-private boundary.
#[async_trait]
pub trait KernelSessionArtifactStore: Send + Sync {
    async fn append_transcript_event(
        &self,
        workspace_root: &Path,
        event: &KernelTranscriptEvent,
    ) -> TalosResult<()>;

    async fn load_transcript_events(
        &self,
        workspace_root: &Path,
        session_id: &str,
        invocation_id: Option<&str>,
        branch: Option<ExecutionBranch>,
    ) -> TalosResult<Vec<KernelTranscriptEvent>>;

    async fn save_summary(
        &self,
        workspace_root: &Path,
        summary: &KernelSessionSummary,
    ) -> TalosResult<()>;

    async fn load_summaries(
        &self,
        workspace_root: &Path,
        session_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<KernelSessionSummary>>;

    async fn session_size_bytes(&self, workspace_root: &Path, session_id: &str)
        -> TalosResult<u64>;

    async fn reset_session(&self, workspace_root: &Path, session_id: &str) -> TalosResult<()>;
}

/// Reads and advances the only product-owned cursor consumed by Kernel memory formation.
#[async_trait]
pub trait KernelMemoryProgressStore: Send + Sync {
    async fn processed_until_event_seq(&self, session_id: &str) -> TalosResult<u64>;

    async fn advance_processed_until_event_seq(
        &self,
        session_id: &str,
        expected_until_seq: u64,
        next_until_seq: u64,
    ) -> TalosResult<()>;
}
