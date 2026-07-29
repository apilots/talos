use talos_store::session::ActiveTurnRecord;
use talos_types::talos::{AgentEvent, AgentEventPayload, AgentEventType, TalosTurnErrorEvent};
use uuid::Uuid;

use super::{ServiceResult, TalosService};

const ORPHAN_TURN_RECOVERY_EVENT_LIMIT: usize = 1024;

impl TalosService {
    pub(super) async fn finalize_orphaned_active_turn(
        &self,
        session_id: &str,
        active_turn: &ActiveTurnRecord,
    ) -> ServiceResult<()> {
        let previous_events = self
            .event_log
            .list_turn_events(
                session_id,
                &active_turn.turn_id,
                ORPHAN_TURN_RECOVERY_EVENT_LIMIT,
            )
            .await?;
        let sequence = previous_events
            .last()
            .map(|event| event.sequence.saturating_add(1))
            .unwrap_or(0);
        let request_id = previous_events
            .iter()
            .find_map(request_id_from_event)
            .map(str::to_string)
            .unwrap_or_else(|| format!("req_orphaned_{}", active_turn.turn_id));
        let message_id = active_turn
            .message_id
            .clone()
            .unwrap_or_else(|| format!("msg_{}", Uuid::new_v4().simple()));
        let failed_at_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
        let error_event = AgentEvent {
            event_seq: None,
            event_id: None,
            created_at_ms: None,
            event_type: AgentEventType::TurnError,
            turn_id: active_turn.turn_id.clone(),
            message_id,
            session_id: session_id.to_string(),
            sequence,
            payload: AgentEventPayload::TurnError(TalosTurnErrorEvent {
                request_id,
                turn_id: active_turn.turn_id.clone(),
                error:
                    "Turn was interrupted because the server restarted before the turn completed."
                        .to_string(),
                error_info: None,
                failed_at_ms,
            }),
        };
        self.event_log.record_talos_event(error_event).await?;
        Ok(())
    }
}

fn request_id_from_event(event: &AgentEvent) -> Option<&str> {
    match &event.payload {
        AgentEventPayload::TurnStarted(payload) => Some(&payload.request_id),
        AgentEventPayload::ItemStarted(payload) => Some(&payload.request_id),
        AgentEventPayload::ItemDelta(payload) => Some(&payload.request_id),
        AgentEventPayload::ItemCompleted(payload) => Some(&payload.request_id),
        AgentEventPayload::AssistantTextCommitted(payload) => Some(&payload.request_id),
        AgentEventPayload::TurnMetrics(payload) => Some(&payload.request_id),
        AgentEventPayload::TurnCompleted(payload) => Some(&payload.request_id),
        AgentEventPayload::TurnError(payload) => Some(&payload.request_id),
    }
}
