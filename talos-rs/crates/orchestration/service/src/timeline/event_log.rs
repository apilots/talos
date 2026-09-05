use std::sync::Arc;

use talos_agent_api::{AgentTurnRef, TimelineEvent};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::session::{AgentActiveTurnRecord, AgentEventRecord, AgentExecutionStore};
use tokio::sync::broadcast;

use super::{mapping::map_timeline_event, TimelineEventId};

const MAX_TIMELINE_READ_LIMIT: usize = 10_000;

/// Service-owned read and publication boundary for committed Agent facts.
///
/// This service never writes execution facts. `AgentExecutionStore` transactions
/// are their unique durable owner; callers may publish only records returned by
/// a successful commit. Gateway and clients receive the resulting
/// `TimelineEvent` without another business-event envelope.
#[derive(Clone)]
pub struct TimelineEventService {
    execution_store: Arc<dyn AgentExecutionStore>,
    live_tx: broadcast::Sender<TimelineEvent>,
}

impl TimelineEventService {
    pub fn new(execution_store: Arc<dyn AgentExecutionStore>) -> Self {
        Self {
            execution_store,
            live_tx: broadcast::channel(1_024).0,
        }
    }

    pub async fn list_events_after(
        &self,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<Vec<TimelineEvent>> {
        let after_event_seq = match after_event_id {
            Some(event_id) => {
                let event_seq = event_id.event_seq()?;
                let cursor_event = self
                    .execution_store
                    .get_agent_event(event_seq)
                    .await?
                    .ok_or_else(expired_cursor_error)?;
                if cursor_event.identity.session_id != session_id {
                    return Err(TalosError::public(
                        ErrorCode::InvalidRequest,
                        "timeline cursor belongs to another Session",
                    ));
                }
                Some(event_seq)
            }
            None => None,
        };
        self.execution_store
            .list_agent_events_after(
                session_id,
                after_event_seq,
                limit.clamp(1, MAX_TIMELINE_READ_LIMIT),
            )
            .await
            .map(|records| records.into_iter().map(map_timeline_event).collect())
    }

    /// Reduce canonical facts through an exact durable Turn tail for history or recovery.
    pub(crate) async fn turn_checkpoint(
        &self,
        identity: &AgentTurnRef,
        through_event_seq: i64,
    ) -> TalosResult<talos_agent::AgentRunCheckpoint> {
        let mut reducer = talos_agent::AgentEventReducer::new(identity.clone());
        let mut found_tail = false;
        super::scan_turn_events(
            self.execution_store.as_ref(),
            identity,
            through_event_seq,
            |record| {
                found_tail = record.event_seq == through_event_seq;
                reducer
                    .apply(map_timeline_event(record).event)
                    .map_err(talos_agent::AgentError::into_talos_error)
            },
        )
        .await?;
        if !found_tail {
            return Err(TalosError::internal(
                "canonical Turn checkpoint tail was not found",
            ));
        }
        Ok(reducer.checkpoint())
    }

    /// Scan one Turn at the current Session tail for a bounded consumer projection.
    pub(crate) async fn scan_turn_events(
        &self,
        identity: &AgentTurnRef,
        mut visit: impl FnMut(TimelineEvent) -> TalosResult<()> + Send,
    ) -> TalosResult<()> {
        let tail = self
            .execution_store
            .get_latest_agent_event_seq(&identity.session_id)
            .await?
            .unwrap_or_default();
        super::scan_turn_events(self.execution_store.as_ref(), identity, tail, |record| {
            visit(map_timeline_event(record))
        })
        .await
    }

    pub async fn latest_event_id(&self, session_id: &str) -> TalosResult<Option<TimelineEventId>> {
        self.execution_store
            .get_latest_agent_event_seq(session_id)
            .await
            .map(|event_seq| event_seq.map(TimelineEventId::from_seq))
    }

    pub(crate) async fn get_event(&self, event_seq: i64) -> TalosResult<Option<TimelineEvent>> {
        self.execution_store
            .get_agent_event(event_seq)
            .await
            .map(|record| record.map(map_timeline_event))
    }

    pub fn subscribe_live_events(&self) -> broadcast::Receiver<TimelineEvent> {
        self.live_tx.subscribe()
    }

    pub(crate) async fn get_active_turn(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<AgentActiveTurnRecord>> {
        self.execution_store.get_agent_active_turn(session_id).await
    }

    pub(crate) async fn get_latest_terminal_event(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<TimelineEvent>> {
        self.execution_store
            .get_latest_agent_terminal_event(session_id)
            .await
            .map(|record| record.map(map_timeline_event))
    }

    pub(crate) async fn list_latest_terminal_events(
        &self,
        session_id: &str,
        before_event_seq: Option<i64>,
        limit: usize,
    ) -> TalosResult<Vec<TimelineEvent>> {
        self.execution_store
            .list_latest_agent_terminal_events(
                session_id,
                before_event_seq,
                limit.clamp(1, MAX_TIMELINE_READ_LIMIT),
            )
            .await
            .map(|records| records.into_iter().map(map_timeline_event).collect())
    }

    /// Publish records returned by one successful Store commit.
    pub(crate) fn publish_committed(
        &self,
        records: impl IntoIterator<Item = AgentEventRecord>,
    ) -> Vec<TimelineEvent> {
        records
            .into_iter()
            .map(|record| {
                let event = map_timeline_event(record);
                let _ = self.live_tx.send(event.clone());
                event
            })
            .collect()
    }
}

fn expired_cursor_error() -> TalosError {
    TalosError::public(
        ErrorCode::ResourceExpired,
        "timeline cursor is not available in canonical Agent history",
    )
}

#[cfg(test)]
#[path = "event_log_tests.rs"]
mod tests;
