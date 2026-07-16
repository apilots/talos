use async_trait::async_trait;
use talos_error::TalosResult;
use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType, TokenUsage};
use talos_types::{SessionConf, DEFAULT_SESSION_TITLE};

use super::{SessionQuery, SessionRecord, SessionStore, SessionSummary};

pub struct MockSessionStore;

impl Default for MockSessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MockSessionStore {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SessionStore for MockSessionStore {
    async fn insert_session_record(
        &self,
        _record: &SessionRecord,
        _agent_id: &str,
        _scope: &str,
    ) -> TalosResult<()> {
        Ok(())
    }

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
        Ok(Some(SessionRecord {
            id: session_id.to_string(),
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
            usage: TokenUsage::default(),
            conf: SessionConf::default(),
            workspace_id: None,
            project_id: None,
            current_directory: None,
        }))
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

    async fn update_session_record(&self, _record: &SessionRecord) -> TalosResult<bool> {
        Ok(true)
    }

    async fn delete_session_record(&self, _session_id: &str) -> TalosResult<bool> {
        Ok(true)
    }
}
