//! Shared synthetic Worker facts for product-side projection tests.
use agent_protocol::{
    AgentCommittedEvent, AgentExecutionPurpose, AgentExecutionSettlement, AgentSessionEventCursor,
    AgentSessionEventsResult,
};
use talos_agent_api::{AgentEvent, AgentEventPayload, AgentMessageRole, AgentRunMetrics};
use talos_error::{TalosError, TalosResult};
use talos_store::session::{
    AgentExecutionStore, CommandLease, DatabaseSessionStore, SessionStore, WorkerProjectionParams,
};

pub(crate) struct TestWorkerEvents {
    pub events: Vec<AgentEvent>,
    pub metrics: Option<AgentRunMetrics>,
    pub command: Option<CommandLease>,
    pub automation_origin: Option<talos_types::AutomationTurnOrigin>,
    pub purpose: AgentExecutionPurpose,
    pub after_cursor: Option<Option<AgentSessionEventCursor>>,
    pub history_supplement: Option<talos_agent_api::trajectory::SessionHistorySupplement>,
}
impl TestWorkerEvents {
    pub(crate) fn new(events: Vec<AgentEvent>) -> Self {
        Self {
            events,
            metrics: None,
            command: None,
            automation_origin: None,
            purpose: AgentExecutionPurpose::Turn,
            after_cursor: None,
            history_supplement: None,
        }
    }
}

pub(crate) use talos_store::session::WorkerProjectionCommit;

pub(crate) trait ProjectTestWorkerEvents {
    fn project_test_events(
        &self,
        batch: TestWorkerEvents,
    ) -> impl std::future::Future<Output = TalosResult<WorkerProjectionCommit>> + Send;
}
impl ProjectTestWorkerEvents for DatabaseSessionStore {
    async fn project_test_events(
        &self,
        batch: TestWorkerEvents,
    ) -> TalosResult<WorkerProjectionCommit> {
        let first = batch
            .events
            .first()
            .ok_or_else(|| TalosError::internal("Worker fixture needs events"))?;
        let session_id = first.identity.session_id.clone();
        let session = self
            .get_session_record(&session_id)
            .await?
            .ok_or_else(|| TalosError::internal("Worker fixture needs a product Session"))?;
        let after_cursor = match batch.after_cursor {
            Some(cursor) => cursor,
            None => self.worker_event_cursor(&session_id).await?,
        };
        let generation = self
            .history_generation(&session_id)
            .await?
            .unwrap_or_default();
        let offset = after_cursor.as_ref().map_or(0, |cursor| cursor.offset);
        let events = batch
            .events
            .into_iter()
            .enumerate()
            .map(|(index, event)| {
                let user_message = match &event.payload {
                    AgentEventPayload::MessageCompleted(message)
                        if message.role == AgentMessageRole::User =>
                    {
                        Some(message)
                    }
                    _ => None,
                };
                let automation_input = user_message.and_then(|message| {
                    batch.automation_origin.clone().map(|origin| {
                        talos_types::AutomationInputOrigin {
                            message_id: message.message_id.clone(),
                            origin,
                        }
                    })
                });
                let command = user_message.and(batch.command.as_ref()).map(|lease| {
                    agent_protocol::AgentCommandIdentity {
                        command: lease.command,
                        idempotency_key: lease.idempotency_key.clone(),
                        request_id: lease.request_id.clone(),
                    }
                });
                let metrics = batch.metrics.clone().unwrap_or_default();
                let settlement = match &event.payload {
                    AgentEventPayload::WaitingForInput(_) => {
                        Some(AgentExecutionSettlement::WaitingForInput { metrics })
                    }
                    payload if payload.is_terminal() => Some(AgentExecutionSettlement::Finished {
                        metrics,
                        memory_usage: vec![],
                    }),
                    _ => None,
                };
                let history_supplement = event
                    .payload
                    .is_terminal()
                    .then(|| batch.history_supplement.clone())
                    .flatten();
                AgentCommittedEvent {
                    cursor: AgentSessionEventCursor {
                        session_id: session_id.clone(),
                        generation,
                        offset: offset + index as i64 + 1,
                    },
                    event,
                    settlement,
                    execution_purpose: batch.purpose,
                    automation_input,
                    command,
                    execution_model: session
                        .conf
                        .model_selection
                        .as_ref()
                        .map(|selection| selection.model_ref.clone()),
                    history_supplement,
                }
            })
            .collect::<Vec<_>>();
        let next_cursor = events
            .last()
            .expect("nonempty Worker fixture")
            .cursor
            .clone();
        let commit = self
            .project_worker_events(WorkerProjectionParams {
                execution_node_id: session.execution_node_id,
                session_id,
                after_cursor,
                page: AgentSessionEventsResult {
                    events,
                    next_cursor,
                    has_more: false,
                    active_turn: None,
                },
            })
            .await?;
        Ok(commit)
    }
}

/// Minimal product binding for tests that only inspect synthetic Worker facts.
pub(crate) fn test_session_record(session_id: &str) -> talos_store::session::SessionRecord {
    talos_store::session::SessionRecord {
        session_kind: talos_types::SessionKind::Standard,
        id: session_id.into(),
        agent_id: "default".into(),
        execution_node_id: "embedded".into(),
        tree_identity: talos_types::SessionTreeIdentity::Primary,
        workspace_id: "default".into(),
        title: talos_types::DEFAULT_SESSION_TITLE.into(),
        user_id: "user-1".into(),
        source_type: talos_types::talos::SessionSourceType::System,
        source_provider: talos_types::talos::SessionSourceProvider::System,
        channel_id: None,
        thread_id: None,
        environment: None,
        created_at: 1,
        last_activity: 1,
        message_count: 0,
        latest_turn_seq: 0,
        memory_processed_until_event_seq: 0,
        usage: talos_types::SessionUsage::default(),
        conf: talos_types::SessionConf::default(),
        project_id: None,
        worktree_binding: None,
        current_directory: None,
        pinned: false,
    }
}
