use async_trait::async_trait;
use talos_error::TalosResult;

use crate::store::{CanonicalTurn, MemoryStorePromptContext, SummaryRecord};

/// Trait for memory implementations
#[async_trait]
pub trait Memory: Send + Sync {
    async fn next_turn_seq(&self, session_id: &str) -> TalosResult<u64>;
    async fn append_turn(&self, turn: &CanonicalTurn) -> TalosResult<()>;
    async fn build_prompt_context(
        &self,
        session_id: &str,
        target_recent_messages: usize,
        include_summary: bool,
        summary_recent_count: usize,
        summary_max_chars: usize,
    ) -> TalosResult<MemoryStorePromptContext>;
    async fn save_summary(&self, record: &SummaryRecord) -> TalosResult<()>;
    async fn load_summaries(
        &self,
        session_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<SummaryRecord>>;
    async fn latest_seq(&self, session_id: &str) -> TalosResult<u64>;
    async fn session_size_bytes(&self, session_id: &str) -> TalosResult<u64>;
}
