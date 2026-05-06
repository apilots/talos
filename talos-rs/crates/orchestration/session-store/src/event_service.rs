use std::sync::Arc;

use talos_error::TalosResult;
use talos_types::talos::TalosEvent;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::active_turn::{
    ActiveTurnStatus, ActiveTurnStore, ActiveTurnUpdate, PendingDynamicToolResult,
    PendingToolApproval, PendingUserAction,
};
use crate::events::{NewSessionEvent, SessionEventRecord, SessionEventStore};

#[derive(Debug, Clone)]
pub struct SessionEventRetentionConfig {
    pub retention_hours: u32,
    pub max_events_per_session: usize,
    pub gc_interval_minutes: u32,
    pub gc_batch_size: usize,
}

impl Default for SessionEventRetentionConfig {
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
pub struct SessionEventService {
    event_store: Arc<dyn SessionEventStore>,
    active_turn_store: Arc<dyn ActiveTurnStore>,
    retention: SessionEventRetentionConfig,
    live_tx: broadcast::Sender<SessionEventRecord>,
}

impl SessionEventService {
    pub fn new(
        event_store: Arc<dyn SessionEventStore>,
        active_turn_store: Arc<dyn ActiveTurnStore>,
        retention: SessionEventRetentionConfig,
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
        after_event_id: Option<&crate::SessionEventId>,
        limit: usize,
    ) -> TalosResult<Vec<SessionEventRecord>> {
        self.event_store
            .list_events_after(session_id, after_event_id, limit)
            .await
    }

    pub async fn latest_event_id(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<crate::SessionEventId>> {
        self.event_store.latest_event_id(session_id).await
    }

    pub async fn oldest_event_id(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<crate::SessionEventId>> {
        self.event_store.oldest_event_id(session_id).await
    }

    pub fn subscribe_live_events(&self) -> broadcast::Receiver<SessionEventRecord> {
        self.live_tx.subscribe()
    }

    pub async fn get_active_turn(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<crate::ActiveTurnRecord>> {
        self.active_turn_store.get_active_turn(session_id).await
    }

    pub async fn record_talos_event(
        &self,
        turn_id: Option<String>,
        event: TalosEvent,
    ) -> TalosResult<SessionEventRecord> {
        let session_id = event.session_id.clone();
        let message_id = event.message_id.clone();
        let active_state = match &event.payload {
            talos_types::talos::TalosEventPayload::MessageCompleted(_) => {
                self.completed_active_state(&session_id, &message_id, turn_id.as_deref())
                    .await?
            }
            talos_types::talos::TalosEventPayload::MessageError(_) => {
                Some((ActiveTurnStatus::Failed, None))
            }
            talos_types::talos::TalosEventPayload::MessageToolCallPendingApproval(payload) => {
                Some((
                    ActiveTurnStatus::WaitingUserAction,
                    Some(PendingUserAction::ToolApproval(PendingToolApproval {
                        call_id: payload.call_id.clone(),
                        tool_name: payload.tool_name.clone(),
                        prompt: payload.prompt.clone(),
                        arguments: payload.arguments.clone(),
                    })),
                ))
            }
            talos_types::talos::TalosEventPayload::MessageToolCallPendingDynamicResult(payload) => {
                Some((
                    ActiveTurnStatus::WaitingUserAction,
                    Some(PendingUserAction::DynamicToolResult(
                        PendingDynamicToolResult {
                            call_id: payload.call_id.clone(),
                            tool_name: payload.tool_name.clone(),
                        },
                    )),
                ))
            }
            _ => Some((ActiveTurnStatus::Running, None)),
        };
        let active_turn_id = turn_id.clone();
        let new_event = NewSessionEvent::from_talos_event(turn_id, event);
        let record = self.event_store.append_event(new_event).await?;
        let _ = self.live_tx.send(record.clone());

        if let (Some(active_turn_id), Some((status, pending_user_action))) =
            (active_turn_id, active_state)
        {
            self.active_turn_store
                .upsert_active_turn(ActiveTurnUpdate {
                    session_id,
                    turn_id: active_turn_id,
                    message_id: Some(message_id),
                    status,
                    last_event_id: Some(record.event_id.clone()),
                    pending_user_action,
                    updated_at: record.created_at,
                })
                .await?;
        }

        Ok(record)
    }

    async fn completed_active_state(
        &self,
        session_id: &str,
        message_id: &str,
        turn_id: Option<&str>,
    ) -> TalosResult<Option<(ActiveTurnStatus, Option<PendingUserAction>)>> {
        let Some(active) = self.active_turn_store.get_active_turn(session_id).await? else {
            return Ok(Some((ActiveTurnStatus::Completed, None)));
        };
        let same_turn = turn_id.is_some_and(|value| value == active.turn_id);
        let same_message = active
            .message_id
            .as_deref()
            .is_some_and(|value| value == message_id);
        if (same_turn || same_message)
            && active.status == ActiveTurnStatus::WaitingUserAction
            && active.pending_user_action.is_some()
        {
            return Ok(Some((
                ActiveTurnStatus::WaitingUserAction,
                active.pending_user_action,
            )));
        }
        Ok(Some((ActiveTurnStatus::Completed, None)))
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
            tracing::warn!(error = %err, "session event log gc failed on startup");
        }

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = interval.tick() => {
                    if let Err(err) = self.run_event_log_gc().await {
                        tracing::warn!(error = %err, "session event log gc failed");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DatabaseSessionStore, SessionEventStore};
    use std::sync::Arc;
    use talos_storage::SqliteStorage;
    use talos_types::talos::{
        StopReason, TalosEventPayload, TalosEventType, TalosMessageCompletedEvent,
        TalosMessageToolCallPendingApprovalEvent, TalosMessageToolCallPendingDynamicResultEvent,
        TalosUsage,
    };

    async fn service() -> (SessionEventService, Arc<DatabaseSessionStore>) {
        let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
        let store = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
        let event_store: Arc<dyn SessionEventStore> = store.clone();
        let active_turn_store: Arc<dyn ActiveTurnStore> = store.clone();
        (
            SessionEventService::new(
                event_store,
                active_turn_store,
                SessionEventRetentionConfig::default(),
            ),
            store,
        )
    }

    #[tokio::test]
    async fn completed_event_updates_active_turn_terminal_status() {
        let (service, store) = service().await;
        let record = service
            .record_talos_event(
                Some("turn_1".to_string()),
                TalosEvent {
                    event_type: TalosEventType::MessageCompleted,
                    message_id: "msg_1".to_string(),
                    session_id: "s1".to_string(),
                    sequence: 1,
                    payload: TalosEventPayload::MessageCompleted(TalosMessageCompletedEvent {
                        request_id: "req_1".to_string(),
                        stop_reason: StopReason::EndTurn,
                        usage: TalosUsage::default(),
                        total_output_items: 1,
                        iteration: 1,
                        step_id: "step-1".to_string(),
                        task_count: 1,
                    }),
                },
            )
            .await
            .unwrap();

        let active = store.get_active_turn("s1").await.unwrap().unwrap();
        assert_eq!(active.status, ActiveTurnStatus::Completed);
        assert_eq!(active.last_event_id, Some(record.event_id));
    }

    #[tokio::test]
    async fn pending_approval_event_updates_active_turn_user_action() {
        let (service, store) = service().await;
        service
            .record_talos_event(
                Some("turn_1".to_string()),
                TalosEvent {
                    event_type: talos_types::talos::TalosEventType::MessageToolCallPendingApproval,
                    message_id: "msg_1".to_string(),
                    session_id: "s1".to_string(),
                    sequence: 1,
                    payload: TalosEventPayload::MessageToolCallPendingApproval(
                        TalosMessageToolCallPendingApprovalEvent {
                            request_id: "req_1".to_string(),
                            call_id: "call_1".to_string(),
                            tool_name: "Bash".to_string(),
                            prompt: "Approve?".to_string(),
                            arguments: serde_json::json!({"cmd": "date"}).into(),
                            step_id: "step-1".to_string(),
                        },
                    ),
                },
            )
            .await
            .unwrap();

        let active = store.get_active_turn("s1").await.unwrap().unwrap();
        assert_eq!(active.status, ActiveTurnStatus::WaitingUserAction);
        assert!(matches!(
            active.pending_user_action,
            Some(PendingUserAction::ToolApproval(ref pending))
                if pending.call_id == "call_1" && pending.tool_name == "Bash"
        ));
    }

    #[tokio::test]
    async fn pending_dynamic_event_updates_active_turn_user_action() {
        let (service, store) = service().await;
        service
            .record_talos_event(
                Some("turn_1".to_string()),
                TalosEvent {
                    event_type:
                        talos_types::talos::TalosEventType::MessageToolCallPendingDynamicResult,
                    message_id: "msg_1".to_string(),
                    session_id: "s1".to_string(),
                    sequence: 1,
                    payload: TalosEventPayload::MessageToolCallPendingDynamicResult(
                        TalosMessageToolCallPendingDynamicResultEvent {
                            request_id: "req_1".to_string(),
                            call_id: "call_1".to_string(),
                            tool_name: "ExternalTool".to_string(),
                            arguments: serde_json::json!({"query": "input"}).into(),
                            step_id: "step-1".to_string(),
                        },
                    ),
                },
            )
            .await
            .unwrap();

        let active = store.get_active_turn("s1").await.unwrap().unwrap();
        assert_eq!(active.status, ActiveTurnStatus::WaitingUserAction);
        assert!(matches!(
            active.pending_user_action,
            Some(PendingUserAction::DynamicToolResult(ref pending))
                if pending.call_id == "call_1" && pending.tool_name == "ExternalTool"
        ));
    }

    #[tokio::test]
    async fn completed_event_preserves_pending_approval_active_turn() {
        let (service, store) = service().await;
        service
            .record_talos_event(
                Some("turn_1".to_string()),
                TalosEvent {
                    event_type: talos_types::talos::TalosEventType::MessageToolCallPendingApproval,
                    message_id: "msg_1".to_string(),
                    session_id: "s1".to_string(),
                    sequence: 1,
                    payload: TalosEventPayload::MessageToolCallPendingApproval(
                        TalosMessageToolCallPendingApprovalEvent {
                            request_id: "req_1".to_string(),
                            call_id: "call_1".to_string(),
                            tool_name: "Bash".to_string(),
                            prompt: "Approve?".to_string(),
                            arguments: serde_json::json!({"cmd": "date"}).into(),
                            step_id: "step-1".to_string(),
                        },
                    ),
                },
            )
            .await
            .unwrap();
        let completed = service
            .record_talos_event(
                Some("turn_1".to_string()),
                TalosEvent {
                    event_type: TalosEventType::MessageCompleted,
                    message_id: "msg_1".to_string(),
                    session_id: "s1".to_string(),
                    sequence: 2,
                    payload: TalosEventPayload::MessageCompleted(TalosMessageCompletedEvent {
                        request_id: "req_1".to_string(),
                        stop_reason: StopReason::EndTurn,
                        usage: TalosUsage::default(),
                        total_output_items: 1,
                        iteration: 1,
                        step_id: "done".to_string(),
                        task_count: 1,
                    }),
                },
            )
            .await
            .unwrap();

        let active = store.get_active_turn("s1").await.unwrap().unwrap();
        assert_eq!(active.status, ActiveTurnStatus::WaitingUserAction);
        assert_eq!(active.last_event_id, Some(completed.event_id));
        assert!(matches!(
            active.pending_user_action,
            Some(PendingUserAction::ToolApproval(ref pending))
                if pending.call_id == "call_1" && pending.tool_name == "Bash"
        ));
    }

    #[tokio::test]
    async fn completed_event_preserves_pending_dynamic_active_turn() {
        let (service, store) = service().await;
        service
            .record_talos_event(
                Some("turn_1".to_string()),
                TalosEvent {
                    event_type:
                        talos_types::talos::TalosEventType::MessageToolCallPendingDynamicResult,
                    message_id: "msg_1".to_string(),
                    session_id: "s1".to_string(),
                    sequence: 1,
                    payload: TalosEventPayload::MessageToolCallPendingDynamicResult(
                        TalosMessageToolCallPendingDynamicResultEvent {
                            request_id: "req_1".to_string(),
                            call_id: "call_1".to_string(),
                            tool_name: "ExternalTool".to_string(),
                            arguments: serde_json::json!({"query": "input"}).into(),
                            step_id: "step-1".to_string(),
                        },
                    ),
                },
            )
            .await
            .unwrap();
        let completed = service
            .record_talos_event(
                Some("turn_1".to_string()),
                TalosEvent {
                    event_type: TalosEventType::MessageCompleted,
                    message_id: "msg_1".to_string(),
                    session_id: "s1".to_string(),
                    sequence: 2,
                    payload: TalosEventPayload::MessageCompleted(TalosMessageCompletedEvent {
                        request_id: "req_1".to_string(),
                        stop_reason: StopReason::EndTurn,
                        usage: TalosUsage::default(),
                        total_output_items: 1,
                        iteration: 1,
                        step_id: "done".to_string(),
                        task_count: 1,
                    }),
                },
            )
            .await
            .unwrap();

        let active = store.get_active_turn("s1").await.unwrap().unwrap();
        assert_eq!(active.status, ActiveTurnStatus::WaitingUserAction);
        assert_eq!(active.last_event_id, Some(completed.event_id));
        assert!(matches!(
            active.pending_user_action,
            Some(PendingUserAction::DynamicToolResult(ref pending))
                if pending.call_id == "call_1" && pending.tool_name == "ExternalTool"
        ));
    }
}
