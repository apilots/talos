use talos_agent_kernel::TimelineBuilder;
use talos_error::TalosError;
use talos_store::session::ActiveTurnRecord;
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
            .first()
            .map(|event| event.request_id().to_string())
            .unwrap_or_else(|| format!("req_orphaned_{}", active_turn.turn_id));
        let message_id = active_turn
            .message_id
            .clone()
            .unwrap_or_else(|| format!("msg_{}", Uuid::new_v4().simple()));
        let mut timeline = TimelineBuilder::new_with_sequence(
            request_id,
            active_turn.turn_id.clone(),
            message_id,
            session_id,
            sequence,
        );
        let error_event = timeline.fail_turn(&TalosError::internal(
            "active turn remained running after process restart",
        ));
        self.event_log.record_talos_event(error_event).await?;
        Ok(())
    }
}
