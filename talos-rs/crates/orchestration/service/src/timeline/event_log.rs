use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use talos_agent_kernel::TimelineBuilder;
use talos_error::TalosResult;
use talos_store::session::{
    ActiveTurnRecord, ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, TimelineEventId,
    TimelineEventStore, UserActionRecord, UserActionStore,
};
use talos_types::talos::{
    AgentEvent, AgentEventPayload, AgentEventType, TalosItemDeltaEvent, TalosTimelineDelta,
    TalosTimelineItemStatus, TimelineEventEnvelope, TokenUsage,
};
use talos_types::{StopReason, TimelineUserActionEvent, UserActionParams, UserActionState};
use tokio::sync::{broadcast, Mutex as AsyncMutex};
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
    session_writes: Arc<Mutex<HashMap<String, Weak<AsyncMutex<()>>>>>,
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
            session_writes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn session_write_lock(&self, session_id: &str) -> Arc<AsyncMutex<()>> {
        let mut locks = self.session_writes.lock().unwrap();
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(session_id).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(AsyncMutex::new(()));
        locks.insert(session_id.to_string(), Arc::downgrade(&lock));
        lock
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

    pub(crate) async fn wait_for_turn_terminal(
        &self,
        session_id: &str,
        turn_id: &str,
        timeout: Duration,
    ) -> TalosResult<bool> {
        let mut events = self.live_tx.subscribe();
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if self
                .active_turn_store
                .get_active_turn(session_id)
                .await?
                .is_some_and(|active| active.turn_id == turn_id && active.status.is_terminal())
            {
                return Ok(true);
            }
            match tokio::time::timeout_at(deadline, events.recv()).await {
                Ok(Ok(_)) | Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
                Ok(Err(broadcast::error::RecvError::Closed)) | Err(_) => return Ok(false),
            }
        }
    }

    /// Validate and persist an interrupt before asking the runtime to stop the turn.
    pub(crate) async fn begin_cancellation(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> TalosResult<ActiveTurnRecord> {
        let write_lock = self.session_write_lock(session_id);
        let _guard = write_lock.lock().await;
        let current = self
            .active_turn_store
            .get_active_turn(session_id)
            .await?
            .filter(|current| current.turn_id == turn_id)
            .ok_or_else(|| {
                talos_error::TalosError::public(
                    talos_error::ErrorCode::ResourceNotFound,
                    format!("active turn `{turn_id}` for session `{session_id}`"),
                )
            })?;
        if current.status.is_terminal() {
            return Ok(current);
        }
        self.active_turn_store
            .upsert_active_turn(ActiveTurnUpdate {
                session_id: current.session_id.clone(),
                turn_id: current.turn_id.clone(),
                message_id: current.message_id.clone(),
                status: ActiveTurnStatus::Cancelling,
                last_event_id: current.last_event_id.clone(),
                current_action_id: current.current_action_id.clone(),
                updated_at: chrono::Utc::now().timestamp_millis(),
            })
            .await?;
        Ok(current)
    }

    /// Restore the previous active state only when runtime cancellation was rejected.
    pub(crate) async fn restore_active_turn_on_cancel_error(
        &self,
        previous: &ActiveTurnRecord,
    ) -> TalosResult<()> {
        let Some(current) = self
            .active_turn_store
            .get_active_turn(&previous.session_id)
            .await?
        else {
            return Ok(());
        };
        if current.turn_id != previous.turn_id || current.status != ActiveTurnStatus::Cancelling {
            return Ok(());
        }
        self.active_turn_store
            .upsert_active_turn(ActiveTurnUpdate {
                session_id: previous.session_id.clone(),
                turn_id: previous.turn_id.clone(),
                message_id: previous.message_id.clone(),
                status: previous.status,
                last_event_id: previous.last_event_id.clone(),
                current_action_id: previous.current_action_id.clone(),
                updated_at: chrono::Utc::now().timestamp_millis(),
            })
            .await
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

    /// Cancel a persisted waiting action and close its item before the Turn.
    pub(crate) async fn cancel_waiting_user_action(
        &self,
        active: ActiveTurnRecord,
    ) -> TalosResult<()> {
        let write_lock = self.session_write_lock(&active.session_id);
        let _guard = write_lock.lock().await;
        let action_id = active.current_action_id.clone().ok_or_else(|| {
            talos_error::TalosError::internal("waiting active turn is missing its user action id")
        })?;
        let record = self
            .user_action_store
            .get(&active.session_id, &active.turn_id, &action_id)
            .await?
            .ok_or_else(|| {
                talos_error::TalosError::internal(format!(
                    "waiting active turn references missing user action {action_id}"
                ))
            })?;
        let record = self.user_actions().cancel(record).await?;
        let item_id = record.action.item_id.clone().ok_or_else(|| {
            talos_error::TalosError::internal("waiting user action is missing its timeline item id")
        })?;
        let previous = self
            .event_store
            .list_turn_events(&active.session_id, &active.turn_id, 1024)
            .await?;
        let has_finished = previous.iter().any(|event| {
            matches!(
                &event.payload,
                AgentEventPayload::ItemDelta(delta)
                    if matches!(
                        &delta.delta,
                        TalosTimelineDelta::UserAction {
                            event: TimelineUserActionEvent::Finished {
                                action_id: finished_action_id,
                                state: UserActionState::Cancelled,
                                ..
                            }
                        } if finished_action_id == &action_id
                    )
            )
        });
        let item_is_terminal = previous.iter().any(|event| {
            matches!(
                &event.payload,
                AgentEventPayload::ItemCompleted(completed) if completed.item_id == item_id
            )
        });
        let turn_is_terminal = previous.iter().any(|event| match &event.payload {
            AgentEventPayload::TurnError(_) => true,
            AgentEventPayload::TurnCompleted(completed) => {
                completed.stop_reason != StopReason::WaitingUserAction
            }
            _ => false,
        });
        let next_sequence = previous
            .last()
            .map(|event| event.sequence.saturating_add(1))
            .unwrap_or_default();
        let request_id = previous
            .first()
            .map(|event| event.request_id().to_string())
            .unwrap_or_else(|| format!("req_cancel_{}", active.turn_id));
        let message_id = active
            .message_id
            .clone()
            .unwrap_or_else(|| format!("msg_cancel_{}", active.turn_id));
        let mut timeline = TimelineBuilder::new_with_sequence(
            request_id,
            active.turn_id.clone(),
            message_id,
            active.session_id.clone(),
            next_sequence,
        );
        timeline.track_existing_tool_call(&item_id, record.action.call_id.clone(), "", "");
        if !has_finished {
            let finished = timeline.append_delta(
                &item_id,
                TalosTimelineDelta::UserAction {
                    event: TimelineUserActionEvent::Finished {
                        action_id: record.action.action_id.clone(),
                        state: UserActionState::Cancelled,
                        response: record.response.clone(),
                    },
                },
            )?;
            self.record_talos_event_inner(finished).await?;
        }
        if !item_is_terminal {
            let item_completed =
                timeline.complete_item(&item_id, TalosTimelineItemStatus::Cancelled, None)?;
            self.record_talos_event_inner(item_completed).await?;
        }
        if !turn_is_terminal {
            let turn_completed =
                timeline.complete_turn(StopReason::Cancelled, TokenUsage::default(), None, None);
            self.record_talos_event_inner(turn_completed).await?;
        }
        Ok(())
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
            return Err(talos_error::TalosError::internal(
                "only a terminal failed user action can finish an active turn",
            ));
        }
        let item_id = record.action.item_id.clone().ok_or_else(|| {
            talos_error::TalosError::internal(
                "recovered user action is missing its timeline item id",
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
            talos_error::TalosError::internal(
                "persisted terminal user action event is missing envelope metadata",
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
        let write_lock = self.session_write_lock(&event.session_id);
        let _guard = write_lock.lock().await;
        self.record_talos_event_inner(event).await
    }

    /// Persist an Agent-forwarded event unless cancellation already owns the terminal path.
    pub(crate) async fn record_forwarded_event(
        &self,
        event: AgentEvent,
    ) -> TalosResult<Option<TimelineEventEnvelope>> {
        let write_lock = self.session_write_lock(&event.session_id);
        let _guard = write_lock.lock().await;
        if self.should_skip_forwarded_event(&event).await? {
            return Ok(None);
        }
        self.record_talos_event_inner(event).await.map(Some)
    }

    async fn should_skip_forwarded_event(&self, event: &AgentEvent) -> TalosResult<bool> {
        let Some(active) = self
            .active_turn_store
            .get_active_turn(&event.session_id)
            .await?
        else {
            return Ok(false);
        };
        if active.turn_id != event.turn_id {
            return Ok(false);
        }
        if active.status.is_terminal() {
            return Ok(true);
        }
        if active.status != ActiveTurnStatus::Cancelling {
            return Ok(false);
        }
        Ok(matches!(
            &event.payload,
            AgentEventPayload::TurnCompleted(completed)
                if completed.stop_reason == StopReason::WaitingUserAction
        ) || matches!(
            &event.payload,
            AgentEventPayload::ItemDelta(delta)
                if matches!(
                    &delta.delta,
                    TalosTimelineDelta::UserAction {
                        event: TimelineUserActionEvent::Requested { .. }
                    }
                )
        ))
    }

    async fn record_talos_event_inner(
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
            talos_error::TalosError::internal(
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
        if !matches!(
            active.status,
            ActiveTurnStatus::WaitingUserAction | ActiveTurnStatus::Cancelling
        ) || active.turn_id != source.turn_id
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
        if record.action.state == UserActionState::Cancelled {
            return Ok(None);
        }
        if active.status == ActiveTurnStatus::Cancelling
            && record.action.state == UserActionState::Pending
        {
            record = self.user_actions().cancel(record).await?;
        } else if record.action.state == UserActionState::Resuming {
            record = self.user_actions().resolve(record).await?;
        } else if !matches!(
            record.action.state,
            UserActionState::Resolved | UserActionState::Cancelled
        ) {
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
                talos_error::TalosError::internal(
                    "persisted user action event is missing envelope metadata",
                )
            })?;
        self.active_turn_store
            .upsert_active_turn(ActiveTurnUpdate {
                session_id: active.session_id,
                turn_id: active.turn_id,
                message_id: active.message_id,
                status: if active.status == ActiveTurnStatus::Cancelling {
                    ActiveTurnStatus::Cancelling
                } else {
                    ActiveTurnStatus::Running
                },
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
