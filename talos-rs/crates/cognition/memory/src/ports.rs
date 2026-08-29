use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use talos_error::TalosResult;
use talos_types::{
    MemoryGetRequest, MemoryGetResponse, MemoryImportance, MemoryKind, MemorySearchRequest,
    MemorySearchResponse,
};

use crate::{DreamMemoryDate, DreamMemoryEntry, MemoryAgentId};

/// Trusted logical workspace, user, and effective root supplied by the runtime or service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryScope {
    pub workspace_id: String,
    pub user_id: String,
    pub workspace_root: PathBuf,
}

/// Read-only long-term memory access used by tools and application services.
pub trait MemoryReader: Send + Sync {
    fn search<'a>(
        &'a self,
        scope: &'a MemoryScope,
        request: MemorySearchRequest,
    ) -> Pin<Box<dyn Future<Output = TalosResult<MemorySearchResponse>> + Send + 'a>>;

    fn get<'a>(
        &'a self,
        scope: &'a MemoryScope,
        request: MemoryGetRequest,
    ) -> Pin<Box<dyn Future<Output = TalosResult<MemoryGetResponse>> + Send + 'a>>;
}

/// Candidate selected by memory formation before persistence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryCandidate {
    pub title: String,
    pub body: String,
    pub kind: MemoryKind,
    pub importance: MemoryImportance,
}

/// One idempotent memory formation result for a contiguous session range.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryWriteBatch {
    pub session_id: String,
    pub occurred_at: i64,
    pub source_from_seq: u64,
    pub source_until_seq: u64,
    pub candidates: Vec<MemoryCandidate>,
}

/// Revision of one date-partitioned Dream Memory document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DreamMemoryDayRevision {
    Missing,
    Present(DreamMemoryRevisionToken),
}

/// Opaque content revision used only for Dream Memory compare-and-swap writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamMemoryRevisionToken(String);

impl DreamMemoryDayRevision {
    /// Computes the revision represented by an exact optional Markdown source.
    #[must_use]
    pub fn from_content(content: Option<&str>) -> Self {
        match content {
            Some(content) => {
                let token = Sha256::digest(content.as_bytes())
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                Self::Present(DreamMemoryRevisionToken(token))
            }
            None => Self::Missing,
        }
    }
}

/// Typed same-day context observed before one Dream Memory formation request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamMemoryDaySnapshot {
    date: DreamMemoryDate,
    revision: DreamMemoryDayRevision,
    entries: Vec<DreamMemoryEntry>,
}

impl DreamMemoryDaySnapshot {
    #[must_use]
    pub const fn new(
        date: DreamMemoryDate,
        revision: DreamMemoryDayRevision,
        entries: Vec<DreamMemoryEntry>,
    ) -> Self {
        Self {
            date,
            revision,
            entries,
        }
    }

    #[must_use]
    pub const fn date(&self) -> DreamMemoryDate {
        self.date
    }

    #[must_use]
    pub fn revision(&self) -> &DreamMemoryDayRevision {
        &self.revision
    }

    #[must_use]
    pub fn entries(&self) -> &[DreamMemoryEntry] {
        &self.entries
    }
}

/// Compare-and-swap append request produced from one observed day snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamMemoryWriteRequest {
    date: DreamMemoryDate,
    base_revision: DreamMemoryDayRevision,
    producer_agent_id: MemoryAgentId,
    batch: MemoryWriteBatch,
}

impl DreamMemoryWriteRequest {
    #[must_use]
    pub fn from_snapshot(
        snapshot: &DreamMemoryDaySnapshot,
        producer_agent_id: MemoryAgentId,
        batch: MemoryWriteBatch,
    ) -> Self {
        Self {
            date: snapshot.date,
            base_revision: snapshot.revision.clone(),
            producer_agent_id,
            batch,
        }
    }

    #[must_use]
    pub const fn date(&self) -> DreamMemoryDate {
        self.date
    }

    #[must_use]
    pub fn base_revision(&self) -> &DreamMemoryDayRevision {
        &self.base_revision
    }

    #[must_use]
    pub fn producer_agent_id(&self) -> &MemoryAgentId {
        &self.producer_agent_id
    }

    #[must_use]
    pub const fn batch(&self) -> &MemoryWriteBatch {
        &self.batch
    }
}

/// Dream Memory formation persistence used by the Agent runtime and maintenance flow.
pub trait DreamMemoryFormationStore: Send + Sync {
    /// Reads the exact typed day state that a formation request will compare against.
    fn read_day<'a>(
        &'a self,
        scope: &'a MemoryScope,
        date: DreamMemoryDate,
    ) -> Pin<Box<dyn Future<Output = TalosResult<DreamMemoryDaySnapshot>> + Send + 'a>>;

    /// Appends a formed batch only when its observed day revision is still current.
    fn write<'a>(
        &'a self,
        scope: &'a MemoryScope,
        request: DreamMemoryWriteRequest,
    ) -> Pin<Box<dyn Future<Output = TalosResult<()>> + Send + 'a>>;
}
