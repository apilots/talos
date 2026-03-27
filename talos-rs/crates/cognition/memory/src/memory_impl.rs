use async_trait::async_trait;
use talos_error::TalosResult;

use crate::retrieval;
use crate::session_store;
use crate::workspace_store;
use crate::store::{
    MemoryGetResult, MemorySearchQuery, MemorySearchResult, MemoryStore, SessionPromptContext,
    SessionPromptContextConfig, SummaryRecord, TalosTurn, WorkspaceMemoryRecord,
};
use crate::Memory;

#[async_trait]
impl Memory for MemoryStore {
    async fn next_turn_seq(&self, session_id: &str) -> TalosResult<u64> {
        session_store::next_turn_seq(self, session_id).await
    }

    async fn append_turn(&self, turn: &TalosTurn) -> TalosResult<()> {
        session_store::append_turn(self, turn).await
    }

    async fn build_session_prompt_context(
        &self,
        session_id: &str,
        config: &SessionPromptContextConfig,
    ) -> TalosResult<SessionPromptContext> {
        session_store::build_session_prompt_context(self, session_id, config).await
    }

    async fn save_summary(&self, record: &SummaryRecord) -> TalosResult<()> {
        session_store::save_summary(self, record).await
    }

    async fn load_summaries(
        &self,
        session_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<SummaryRecord>> {
        session_store::load_summaries(self, session_id, limit).await
    }

    async fn latest_seq(&self, session_id: &str) -> TalosResult<u64> {
        session_store::latest_seq(self, session_id).await
    }

    async fn session_size_bytes(&self, session_id: &str) -> TalosResult<u64> {
        session_store::session_size_bytes(self, session_id).await
    }

    async fn search_memory(
        &self,
        query: &MemorySearchQuery,
    ) -> TalosResult<Vec<MemorySearchResult>> {
        retrieval::search_memory(self, query).await
    }

    async fn get_memory(
        &self,
        path: &str,
        from: Option<usize>,
        lines: Option<usize>,
    ) -> TalosResult<MemoryGetResult> {
        retrieval::get_memory(self, path, from, lines).await
    }

    async fn append_workspace_memory(&self, record: &WorkspaceMemoryRecord) -> TalosResult<()> {
        workspace_store::append_workspace_memory(self, record).await
    }
}
