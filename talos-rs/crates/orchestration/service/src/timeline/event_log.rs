use std::sync::Arc;

use talos_error::TalosResult;
use talos_store::session::{
    ActiveTurnRecord, ActiveTurnStore, ActiveTurnUpdate, TimelineEventId, TimelineEventStore,
    UserActionRecord, UserActionStore,
};
use talos_types::talos::{
    AgentEvent, AgentEventPayload, AgentEventType, TalosItemDeltaEvent, TalosTimelineDelta,
    TimelineEventEnvelope,
};
use talos_types::{TimelineUserActionEvent, UserActionParams, UserActionState};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use super::active_turn::{resolve_pending_user_action_update, state_for_event};
use crate::user_action::UserActionService;

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
    user_action_store: Arc<dyn UserActionStore>,
    retention: TimelineEventRetentionConfig,
    live_tx: broadcast::Sender<TimelineEventEnvelope>,
}

impl TimelineEventService {
    pub fn new(
        event_store: Arc<dyn TimelineEventStore>,
        active_turn_store: Arc<dyn ActiveTurnStore>,
        user_action_store: Arc<dyn UserActionStore>,
        retention: TimelineEventRetentionConfig,
    ) -> Self {
        Self {
            event_store,
            active_turn_store,
            user_action_store,
            retention,
            live_tx: broadcast::channel(1024).0,
        }
    }

    pub async fn list_events_after(
        &self,
        session_id: &str,
        after_event_id: Option<&TimelineEventId>,
        limit: usize,
    ) -> TalosResult<Vec<AgentEvent>> {
        self.event_store
            .list_events_after(session_id, after_event_id, limit)
            .await
    }

    pub(crate) async fn list_turn_events(
        &self,
        session_id: &str,
        turn_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<AgentEvent>> {
        self.event_store
            .list_turn_events(session_id, turn_id, limit)
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

    pub(crate) async fn get_active_turn(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<ActiveTurnRecord>> {
        self.active_turn_store.get_active_turn(session_id).await
    }

    pub(crate) async fn get_current_user_action(
        &self,
        active: &ActiveTurnRecord,
    ) -> TalosResult<Option<talos_types::PendingUserAction>> {
        let Some(action_id) = active.current_action_id.as_deref() else {
            return Ok(None);
        };
        Ok(self
            .user_action_store
            .get(&active.session_id, &active.turn_id, action_id)
            .await?
            .map(|record| record.action))
    }

    pub(crate) fn user_actions(&self) -> UserActionService {
        UserActionService::new(self.user_action_store.clone())
    }

    pub(crate) async fn finish_terminal_user_action(
        &self,
        active: ActiveTurnRecord,
        record: UserActionRecord,
    ) -> TalosResult<()> {
        if !matches!(
            record.action.state,
            UserActionState::ResumeFailed
                | UserActionState::ExecutionUnknown
                | UserActionState::Cancelled
        ) {
            return Err(talos_error::TalosError::ValidationError(
                "only a terminal failed user action can finish an active turn".to_string(),
            ));
        }
        let item_id = record.action.item_id.clone().ok_or_else(|| {
            talos_error::TalosError::ValidationError(
                "recovered user action is missing its timeline item id".to_string(),
            )
        })?;
        let previous = self
            .event_store
            .list_turn_events(&active.session_id, &active.turn_id, 1024)
            .await?;
        let sequence = previous
            .last()
            .map(|event| event.sequence.saturating_add(1))
            .unwrap_or_default();
        let response = if matches!(record.action.params, UserActionParams::DynamicToolResult(_)) {
            None
        } else {
            record.response.clone()
        };
        let event = AgentEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: None,
            event_type: AgentEventType::ItemDelta,
            turn_id: active.turn_id.clone(),
            message_id: active
                .message_id
                .clone()
                .unwrap_or_else(|| format!("msg_recovery_{}", record.action.action_id)),
            session_id: active.session_id.clone(),
            sequence,
            payload: AgentEventPayload::ItemDelta(TalosItemDeltaEvent {
                request_id: format!("req_recovery_{}", record.action.action_id),
                turn_id: active.turn_id.clone(),
                item_id,
                delta: TalosTimelineDelta::UserAction {
                    event: TimelineUserActionEvent::Finished {
                        action_id: record.action.action_id,
                        state: record.action.state,
                        response,
                    },
                },
            }),
        };
        let persisted = self.event_store.append_event(event).await?;
        let envelope = TimelineEventEnvelope::from_persisted_event(persisted).ok_or_else(|| {
            talos_error::TalosError::SerializationError(
                "persisted terminal user action event is missing envelope metadata".to_string(),
            )
        })?;
        self.active_turn_store
            .upsert_active_turn(ActiveTurnUpdate {
                session_id: active.session_id,
                turn_id: active.turn_id,
                message_id: active.message_id,
                status: talos_store::session::ActiveTurnStatus::Failed,
                last_event_id: Some(TimelineEventId::from(envelope.event_id.clone())),
                current_action_id: None,
                updated_at: envelope.created_at_ms,
            })
            .await?;
        let _ = self.live_tx.send(envelope);
        Ok(())
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
        event: AgentEvent,
    ) -> TalosResult<TimelineEventEnvelope> {
        if matches!(event.payload, AgentEventPayload::TurnMetrics(_)) {
            let envelope = live_only_event_envelope(event);
            let _ = self.live_tx.send(envelope.clone());
            return Ok(envelope);
        }

        let source_event = event.clone();
        let session_id = event.session_id.clone();
        let message_id = event.message_id.clone();
        let turn_id = event.turn_id.clone();
        let active_state = state_for_event(
            self.active_turn_store.as_ref(),
            self.user_action_store.as_ref(),
            &event,
        )
        .await?;
        if let Some(new_action) = active_state
            .as_ref()
            .and_then(|state| state.new_action.clone())
        {
            self.user_action_store.create(new_action).await?;
        }
        let record = self.event_store.append_event(event).await?;
        let envelope = TimelineEventEnvelope::from_persisted_event(record).ok_or_else(|| {
            talos_error::TalosError::SerializationError(
                "persisted timeline event is missing envelope metadata".to_string(),
            )
        })?;

        if let Some(active_state) = active_state {
            self.active_turn_store
                .upsert_active_turn(ActiveTurnUpdate {
                    session_id,
                    turn_id,
                    message_id: Some(message_id),
                    status: active_state.status,
                    last_event_id: Some(TimelineEventId::from(envelope.event_id.clone())),
                    current_action_id: active_state.current_action_id,
                    updated_at: envelope.created_at_ms,
                })
                .await?;
        }

        let _ = self.live_tx.send(envelope.clone());
        if let Some(finished) = self.finish_user_action_for_event(&source_event).await? {
            let _ = self.live_tx.send(finished);
        }

        Ok(envelope)
    }

    async fn finish_user_action_for_event(
        &self,
        source: &AgentEvent,
    ) -> TalosResult<Option<TimelineEventEnvelope>> {
        let (request_id, item_id) = match &source.payload {
            AgentEventPayload::ItemDelta(payload)
                if matches!(payload.delta, TalosTimelineDelta::ToolResult { .. }) =>
            {
                (&payload.request_id, &payload.item_id)
            }
            AgentEventPayload::ItemCompleted(payload) => (&payload.request_id, &payload.item_id),
            _ => return Ok(None),
        };
        let Some(active) = self
            .active_turn_store
            .get_active_turn(&source.session_id)
            .await?
        else {
            return Ok(None);
        };
        if active.status != talos_store::session::ActiveTurnStatus::WaitingUserAction
            || active.turn_id != source.turn_id
        {
            return Ok(None);
        }
        let Some(action_id) = active.current_action_id.as_deref() else {
            return Ok(None);
        };
        let Some(mut record) = self
            .user_action_store
            .get(&source.session_id, &source.turn_id, action_id)
            .await?
        else {
            return Ok(None);
        };
        if record.action.item_id.as_deref() != Some(item_id.as_str()) {
            return Ok(None);
        }
        if record.action.state == UserActionState::Resuming {
            record = self.user_actions().resolve(record).await?;
        } else if record.action.state != UserActionState::Resolved {
            return Ok(None);
        }
        let response = if matches!(record.action.params, UserActionParams::DynamicToolResult(_)) {
            None
        } else {
            record.response.clone()
        };
        let finished_event = AgentEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: None,
            event_type: AgentEventType::ItemDelta,
            turn_id: source.turn_id.clone(),
            message_id: source.message_id.clone(),
            session_id: source.session_id.clone(),
            sequence: source.sequence.saturating_add(1),
            payload: AgentEventPayload::ItemDelta(TalosItemDeltaEvent {
                request_id: request_id.clone(),
                turn_id: source.turn_id.clone(),
                item_id: item_id.clone(),
                delta: TalosTimelineDelta::UserAction {
                    event: TimelineUserActionEvent::Finished {
                        action_id: record.action.action_id,
                        state: record.action.state,
                        response,
                    },
                },
            }),
        };
        let persisted = self.event_store.append_event(finished_event).await?;
        let finished_envelope =
            TimelineEventEnvelope::from_persisted_event(persisted).ok_or_else(|| {
                talos_error::TalosError::SerializationError(
                    "persisted user action event is missing envelope metadata".to_string(),
                )
            })?;
        self.active_turn_store
            .upsert_active_turn(ActiveTurnUpdate {
                session_id: active.session_id,
                turn_id: active.turn_id,
                message_id: active.message_id,
                status: talos_store::session::ActiveTurnStatus::Running,
                last_event_id: Some(TimelineEventId::from(finished_envelope.event_id.clone())),
                current_action_id: None,
                updated_at: finished_envelope.created_at_ms,
            })
            .await?;
        Ok(Some(finished_envelope))
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

fn live_only_event_envelope(mut event: AgentEvent) -> TimelineEventEnvelope {
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
