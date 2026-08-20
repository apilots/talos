use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use talos_error::TalosResult;
use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType};
use talos_types::{SessionConf, DEFAULT_SESSION_TITLE};

use super::{SessionQuery, SessionRecord, SessionStore, SessionSummary, SessionTreeIdentity};

pub struct MockSessionStore {
    memory_processed_until_seq: AtomicU64,
}

impl Default for MockSessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MockSessionStore {
    pub fn new() -> Self {
        Self {
            memory_processed_until_seq: AtomicU64::new(0),
        }
    }
}

#[async_trait]
impl SessionStore for MockSessionStore {
    async fn insert_session_record(
        &self,
        _record: &SessionRecord,
        _scope: &str,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
        Ok(Some(SessionRecord {
            id: session_id.to_string(),
            agent_id: "default".to_string(),
            tree_identity: SessionTreeIdentity::Primary,
            title: DEFAULT_SESSION_TITLE.to_string(),
            user_id: "mock-user".to_string(),
            context: TalosContext {
                source_type: TalosSourceType::System,
                provider: TalosProvider::System,
                user_id: "mock-user".to_string(),
                message_id: "message-mock".to_string(),
                trace_id: "trace-mock".to_string(),
                channel_id: Some("default".to_string()),
                thread_id: None,
                project_root: None,
                environment: None,
            },
            model: "default".to_string(),
            created_at: 0,
            last_activity: 0,
            message_count: 0,
            latest_turn_seq: self.memory_processed_until_seq.load(Ordering::SeqCst),
            memory_processed_until_seq: self.memory_processed_until_seq.load(Ordering::SeqCst),
            usage: talos_types::SessionUsage::default(),
            conf: SessionConf::default(),
            workspace_id: None,
            project_id: None,
            current_directory: None,
        }))
    }

    async fn get_subagent_record(
        &self,
        _tree_root_session_id: &str,
        _agent_path: &talos_types::AgentPath,
    ) -> TalosResult<Option<SessionRecord>> {
        Ok(None)
    }

    async fn list_session_records(&self, _query: &SessionQuery) -> TalosResult<Vec<SessionRecord>> {
        Ok(Vec::new())
    }

    async fn count_session_records(&self, _query: &SessionQuery) -> TalosResult<usize> {
        Ok(0)
    }

    async fn summarize_sessions(&self, _user_id: &str) -> TalosResult<SessionSummary> {
        Ok(SessionSummary::default())
    }

    async fn has_active_subagent_turn(&self, _tree_root_session_id: &str) -> TalosResult<bool> {
        Ok(false)
    }

    async fn update_session_record(&self, _record: &SessionRecord) -> TalosResult<bool> {
        Ok(true)
    }

    async fn update_context_usage(
        &self,
        _session_id: &str,
        _context_usage: &talos_types::ContextUsage,
    ) -> TalosResult<bool> {
        Ok(true)
    }

    async fn advance_memory_processed_until_seq(
        &self,
        _session_id: &str,
        expected_until_seq: u64,
        next_until_seq: u64,
    ) -> TalosResult<()> {
        match self.memory_processed_until_seq.compare_exchange(
            expected_until_seq,
            next_until_seq,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => Ok(()),
            Err(current) if current >= next_until_seq => Ok(()),
            Err(current) => Err(talos_error::TalosError::public(
                talos_error::ErrorCode::Conflict,
                format!("memory cursor conflict: expected {expected_until_seq}, current {current}"),
            )),
        }
    }

    async fn delete_session_record(&self, _session_id: &str) -> TalosResult<bool> {
        Ok(true)
    }

    async fn delete_session_tree(&self, _tree_root_session_id: &str) -> TalosResult<bool> {
        Ok(true)
    }
}
