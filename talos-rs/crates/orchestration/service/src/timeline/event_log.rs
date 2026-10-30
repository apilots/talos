use std::sync::Arc;

use talos_agent_api::{AgentTurnRef, TimelineEvent};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_store::session::{AgentActiveTurnRecord, AgentEventRecord, AgentExecutionStore};
use tokio::sync::broadcast;

use super::{
    mapping::{map_timeline_event, map_timeline_record},
    TimelineEventId,
};

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
    pub(super) active_tx: broadcast::Sender<talos_agent_api::SessionActiveTurnChangedNotification>,
}

impl TimelineEventService {
    pub(crate) fn supports_history_supplements(&self) -> bool {
        self.execution_store.supports_history_supplements()
    }
    pub(crate) async fn append_history_supplement(
        &self,
        value: talos_agent_api::trajectory::SessionHistorySupplement,
    ) -> TalosResult<()> {
        self.execution_store.append_history_supplement(value).await
    }
    pub(crate) async fn read_terminal_history_supplement(
        &self,
        identity: &AgentTurnRef,
    ) -> TalosResult<Option<talos_agent_api::trajectory::SessionHistorySupplement>> {
        self.execution_store
            .read_terminal_history_supplement(identity)
            .await
    }

    pub(crate) async fn read_execution_payload(
        &self,
        identity: &AgentTurnRef,
        payload_id: &str,
    ) -> TalosResult<Option<String>> {
        self.execution_store
            .read_execution_payload(identity, payload_id)
            .await
    }
    pub fn new(execution_store: Arc<dyn AgentExecutionStore>) -> Self {
        Self {
            execution_store,
            live_tx: broadcast::channel(1_024).0,
            active_tx: broadcast::channel(128).0,
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
                    .get_timeline_event(event_seq)
                    .await?
                    .ok_or_else(expired_cursor_error)?;
                if cursor_event.event.identity().session_id != session_id {
                    return Err(TalosError::new(
                        ErrorCode::InvalidCursor,
                        "timeline cursor belongs to another Session",
                    ));
                }
                Some(event_seq)
            }
            None => None,
        };
        self.execution_store
            .list_timeline_events_after(
                session_id,
                after_event_seq.unwrap_or_default(),
                limit.clamp(1, MAX_TIMELINE_READ_LIMIT),
            )
            .await
            .map(|records| records.into_iter().map(map_timeline_record).collect())
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
                reducer.apply(talos_agent_api::AgentEvent {
                    identity: record.identity,
                    sequence: record.sequence,
                    occurred_at: record.occurred_at,
                    payload: record.payload,
                })
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
            .get_latest_timeline_event_seq(session_id)
            .await
            .map(|event_seq| event_seq.map(TimelineEventId::from_seq))
    }

    pub(crate) async fn turn_notices(
        &self,
        identities: &[AgentTurnRef],
        through_event_seq: i64,
    ) -> TalosResult<Vec<TimelineEvent>> {
        self.execution_store
            .list_turn_notices(identities, through_event_seq)
            .await
            .map(|records| records.into_iter().map(map_timeline_record).collect())
    }

    pub(crate) async fn get_event(&self, event_seq: i64) -> TalosResult<Option<TimelineEvent>> {
        self.execution_store
            .get_timeline_event(event_seq)
            .await
            .map(|record| record.map(map_timeline_record))
    }

    pub fn subscribe_live_events(&self) -> broadcast::Receiver<TimelineEvent> {
        self.live_tx.subscribe()
    }

    /// Wake existing subscribers after synchronized facts commit. The durable
    /// local Timeline remains authoritative; its existing gap replay fills all
    /// intervening records instead of publishing a second business event model.
    pub async fn publish_imported(&self, session_id: &str) -> TalosResult<()> {
        if let Some(seq) = self
            .execution_store
            .get_latest_timeline_event_seq(session_id)
            .await?
        {
            if let Some(record) = self.execution_store.get_timeline_event(seq).await? {
                let _ = self.live_tx.send(map_timeline_record(record));
            }
        }
        self.publish_active_turn(session_id).await
    }

    pub(crate) async fn publish_active_turn(&self, session_id: &str) -> TalosResult<()> {
        let active_turn = self.display_active_turn(session_id).await?;
        let latest_event_id = self
            .latest_event_id(session_id)
            .await?
            .map(|id| id.to_string());
        let _ = self
            .active_tx
            .send(talos_agent_api::SessionActiveTurnChangedNotification {
                session_id: session_id.into(),
                state: talos_agent_api::SessionActiveTurnResponse {
                    active_turn,
                    latest_event_id,
                },
            });
        Ok(())
    }

    /// Commit a product notice before publishing it on the existing Session stream.
    pub(crate) async fn publish_notice(
        &self,
        notice: talos_agent_api::TimelineSystemNotice,
    ) -> TalosResult<TimelineEvent> {
        let record = self.execution_store.append_timeline_notice(notice).await?;
        let event = map_timeline_record(record);
        // No current subscribers is normal; replay retains the committed notice.
        let _ = self.live_tx.send(event.clone());
        Ok(event)
    }

    pub(crate) async fn display_active_turn(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<talos_agent_api::TimelineActiveTurn>> {
        if let Some(active) = self.get_active_turn(session_id).await? {
            return Ok(Some(super::mapping::map_timeline_active_turn(active)));
        }
        self.execution_store
            .get_shared_active_turn(session_id)
            .await
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
    TalosError::new(
        ErrorCode::EventCursorExpired,
        "timeline cursor is not available in canonical Agent history",
    )
}

#[cfg(test)]
#[path = "event_log_tests.rs"]
mod tests;
