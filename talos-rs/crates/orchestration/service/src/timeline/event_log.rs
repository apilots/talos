use std::sync::Arc;

use talos_error::TalosResult;
use talos_store::session::{
    ActiveTurnRecord, ActiveTurnStore, ActiveTurnUpdate, TimelineEventId, TimelineEventStore,
};
use talos_types::talos::{TalosTimelineEvent, TalosTimelineEventPayload, TimelineEventEnvelope};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use super::active_turn::{resolve_pending_user_action_update, state_for_event};

#[derive(Debug, Clone)]
pub struct TimelineEventRetentionConfig {
    pub retention_hours: u32,
    pub max_events_per_session: usize,
    pub gc_interval_minutes: u32,
    pub gc_batch_size: usize,
}

impl Default for TimelineEventRetentionConfig {
    fn default() -> Self {
        Self {
            retention_hours: 24,
            max_events_per_session: 10_000,
            gc_interval_minutes: 30,
            gc_batch_size: 1_000,
        }
    }
}

#[derive(Clone)]
pub struct TimelineEventService {
    event_store: Arc<dyn TimelineEventStore>,
    active_turn_store: Arc<dyn ActiveTurnStore>,
    retention: TimelineEventRetentionConfig,
    live_tx: broadcast::Sender<TimelineEventEnvelope>,
}

impl TimelineEventService {
    pub fn new(
        event_store: Arc<dyn TimelineEventStore>,
        active_turn_store: Arc<dyn ActiveTurnStore>,
        retention: TimelineEventRetentionConfig,
    ) -> Self {
        Self {
            event_store,
            active_turn_store,
            retention,
            live_tx: broadcast::channel(1024).0,
        }
    }

    pub async fn list_events_after(
        &self,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<Vec<TalosTimelineEvent>> {
        self.event_store
            .list_events_after(session_id, after_event_id, limit)
            .await
    }

    pub async fn latest_event_id(&self, session_id: &str) -> TalosResult<Option<TimelineEventId>> {
        self.event_store.latest_event_id(session_id).await
    }

    pub async fn oldest_event_id(&self, session_id: &str) -> TalosResult<Option<TimelineEventId>> {
        self.event_store.oldest_event_id(session_id).await
    }

    pub fn subscribe_live_events(&self) -> broadcast::Receiver<TimelineEventEnvelope> {
        self.live_tx.subscribe()
    }

    pub async fn get_active_turn(&self, session_id: &str) -> TalosResult<Option<ActiveTurnRecord>> {
        self.active_turn_store.get_active_turn(session_id).await
    }

    pub async fn resolve_pending_user_action(&self, session_id: &str) -> TalosResult<()> {
        if let Some(update) =
            resolve_pending_user_action_update(self.active_turn_store.as_ref(), session_id).await?
        {
            self.active_turn_store.upsert_active_turn(update).await?;
        }
        Ok(())
    }

    pub async fn record_talos_event(
        &self,
        event: TalosTimelineEvent,
    ) -> TalosResult<TimelineEventEnvelope> {
        if matches!(event.payload, TalosTimelineEventPayload::TurnMetrics(_)) {
            let envelope = live_only_event_envelope(event);
            let _ = self.live_tx.send(envelope.clone());
            return Ok(envelope);
        }

        let session_id = event.session_id.clone();
        let message_id = event.message_id.clone();
        let turn_id = event.turn_id.clone();
        let active_state = state_for_event(self.active_turn_store.as_ref(), &event).await?;
        let record = self.event_store.append_event(event).await?;
        let envelope = TimelineEventEnvelope::from_persisted_event(record).ok_or_else(|| {
            talos_error::TalosError::SerializationError(
                "persisted timeline event is missing envelope metadata".to_string(),
            )
        })?;

        if let Some((status, pending_user_action)) = active_state {
            self.active_turn_store
                .upsert_active_turn(ActiveTurnUpdate {
                    session_id,
                    turn_id,
                    message_id: Some(message_id),
                    status,
                    last_event_id: Some(TimelineEventId::from(envelope.event_id.clone())),
                    pending_user_action,
                    updated_at: envelope.created_at_ms,
                })
                .await?;
        }

        let _ = self.live_tx.send(envelope.clone());

        Ok(envelope)
    }

    pub async fn run_event_log_gc(&self) -> TalosResult<u64> {
        let cutoff = chrono::Utc::now()
            .timestamp_millis()
            .saturating_sub((self.retention.retention_hours as i64) * 60 * 60 * 1000);
        let deleted_by_age = self
            .event_store
            .delete_completed_events_before(cutoff, self.retention.gc_batch_size)
            .await?;
        let deleted_by_size = self
            .event_store
            .delete_overflow_events_per_session(
                self.retention.max_events_per_session,
                self.retention.gc_batch_size,
            )
            .await?;
        Ok(deleted_by_age + deleted_by_size)
    }

    pub async fn run_event_log_gc_loop(&self, shutdown: CancellationToken) {
        let interval_minutes = self.retention.gc_interval_minutes.max(1) as u64;
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(interval_minutes * 60));

        if let Err(err) = self.run_event_log_gc().await {
            tracing::warn!(error = %err, "timeline event log gc failed on startup");
        }

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = interval.tick() => {
                    if let Err(err) = self.run_event_log_gc().await {
                        tracing::warn!(error = %err, "timeline event log gc failed");
                    }
                }
            }
        }
    }
}

fn live_only_event_envelope(mut event: TalosTimelineEvent) -> TimelineEventEnvelope {
    let now = chrono::Utc::now().timestamp_millis();
    let event_id = format!("live_metrics_{}_{}_{}", event.turn_id, event.sequence, now);
    event.event_id = Some(event_id.clone());
    event.event_seq = Some(now);
    event.created_at_ms = Some(now);
    TimelineEventEnvelope {
        event_id,
        event_seq: now,
        created_at_ms: now,
        session_id: event.session_id.clone(),
        turn_id: event.turn_id.clone(),
        message_id: event.message_id.clone(),
        event,
    }
}

#[cfg(test)]
#[path = "event_log_tests.rs"]
mod tests;
