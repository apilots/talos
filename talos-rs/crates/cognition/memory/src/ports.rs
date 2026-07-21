use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use talos_error::TalosResult;
use talos_types::{
    MemoryGetRequest, MemoryGetResponse, MemoryImportance, MemoryKind, MemorySearchRequest,
    MemorySearchResponse,
};

/// Internal user and workspace boundary supplied by the runtime or service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryScope {
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

/// Long-term memory persistence used by the agent runtime and maintenance flow.
pub trait MemoryWriter: Send + Sync {
    fn write<'a>(
        &'a self,
        scope: &'a MemoryScope,
        batch: MemoryWriteBatch,
    ) -> Pin<Box<dyn Future<Output = TalosResult<()>> + Send + 'a>>;
}
