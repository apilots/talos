//! Finish committed product effects before removing their execution source or identity.
use super::{SessionRecord, TalosError, TalosResult, TurnService};
use talos_error::ErrorCode;

impl TurnService {
    /// The caller holds the product's admission guard and verifies Worker idleness.
    /// Preparation applies existing receipts without dispatching follow-up work.
    pub(crate) async fn prepare_session_destruction(
        &self,
        session: &SessionRecord,
    ) -> TalosResult<()> {
        let source = self.execution.sessions.ensure_worker_idle(session).await?;
        let generation = self
            .execution
            .sessions
            .product_history_generation(&session.id)
            .await?;
        if source.history_generation == generation {
            let context = self
                .execution
                .sessions
                .build_agent_session_context(session)
                .await?;
            self.execution.project_worker(&context).await?;
        } else if generation.checked_add(1) != Some(source.history_generation)
            || source.completed_turn_id.is_some()
            || source.completed_event_sequence != 0
        {
            return Err(TalosError::new(
                ErrorCode::EventCursorExpired,
                "Worker history changed before Session cleanup could be confirmed.",
            ));
        }
        // A next-generation empty source is the existing clear/retire retry contract:
        // a previous preflight finished before its product transaction committed.
        self.settle_product_before_destruction(&session.id).await
    }

    /// Also used after permanent node removal, when only retained product receipts exist.
    pub(crate) async fn settle_product_before_destruction(
        &self,
        session_id: &str,
    ) -> TalosResult<()> {
        loop {
            let before = self
                .execution
                .execution_store
                .pending_worker_settlements(session_id)
                .await?;
            if before.is_empty() {
                return Ok(());
            }
            self.recover_worker_settlements(session_id, /*dispatch*/ false)
                .await?;
            let after = self
                .execution
                .execution_store
                .pending_worker_settlements(session_id)
                .await?;
            if after
                .iter()
                .any(|remaining| before.iter().any(|prior| prior.cursor == remaining.cursor))
            {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "Session product settlement must finish before clearing or deleting its history.",
                ));
            }
        }
    }
}
