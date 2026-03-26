use async_trait::async_trait;
use talos_error::TalosResult;

use crate::store::{
    MemoryGetResult, MemorySearchQuery, MemorySearchResult, RootMemoryContext,
    SessionPromptContext, SessionPromptContextConfig, SummaryRecord, TalosTurn,
    WorkspaceMemoryRecord,
};

/// Trait for memory implementations
#[async_trait]
pub trait Memory: Send + Sync {
    async fn next_turn_seq(&self, session_id: &str) -> TalosResult<u64>;
    async fn append_turn(&self, turn: &TalosTurn) -> TalosResult<()>;
    async fn build_session_prompt_context(
        &self,
        session_id: &str,
        config: &SessionPromptContextConfig,
    ) -> TalosResult<SessionPromptContext>;
    async fn save_summary(&self, record: &SummaryRecord) -> TalosResult<()>;
    async fn load_summaries(
        &self,
        session_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<SummaryRecord>>;
    async fn latest_seq(&self, session_id: &str) -> TalosResult<u64>;
    async fn session_size_bytes(&self, session_id: &str) -> TalosResult<u64>;
    async fn load_root_memory(&self) -> TalosResult<RootMemoryContext>;
    async fn search_memory(
        &self,
        query: &MemorySearchQuery,
    ) -> TalosResult<Vec<MemorySearchResult>>;
    async fn get_memory(
        &self,
        path: &str,
        from: Option<usize>,
        lines: Option<usize>,
    ) -> TalosResult<MemoryGetResult>;
    async fn append_workspace_memory(&self, record: &WorkspaceMemoryRecord) -> TalosResult<()>;
}
