use std::sync::Arc;

use async_trait::async_trait;
use talos_agent_provider_kernel::KernelMemoryProgressStore;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::session::SessionStore;

/// Application composition adapter from durable Session state to Kernel memory progress.
pub(super) struct SessionMemoryProgress {
    sessions: Arc<dyn SessionStore>,
}

impl SessionMemoryProgress {
    pub(super) fn new(sessions: Arc<dyn SessionStore>) -> Self {
        Self { sessions }
    }
}

#[async_trait]
impl KernelMemoryProgressStore for SessionMemoryProgress {
    async fn processed_until_event_seq(&self, session_id: &str) -> TalosResult<u64> {
        self.sessions
            .get_session_record(session_id)
            .await?
            .map(|record| record.memory_processed_until_event_seq)
            .ok_or_else(|| TalosError::public(ErrorCode::ResourceNotFound, session_id.to_string()))
    }

    async fn advance_processed_until_event_seq(
        &self,
        session_id: &str,
        expected_until_seq: u64,
        next_until_seq: u64,
    ) -> TalosResult<()> {
        self.sessions
            .advance_memory_processed_until_event_seq(
                session_id,
                expected_until_seq,
                next_until_seq,
            )
            .await
    }
}
