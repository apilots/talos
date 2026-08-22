use super::TalosService;
use crate::{Actor, ServiceResult};
use std::time::Duration;
use talos_error::TalosError;
use talos_store::session::ActiveTurnStatus;
use talos_types::{CancelActiveTurnOutcome, InterruptTurnRequest, InterruptTurnResponse};

const CANCEL_TERMINAL_WAIT: Duration = Duration::from_secs(2);

impl TalosService {
    pub async fn cancel_active_turn(
        &self,
        actor: &Actor,
        session_id: &str,
        turn_id: &str,
    ) -> ServiceResult<()> {
        let _session = self.access.get_session(actor, session_id).await?;
        let active_turn = self
            .event_log
            .begin_cancellation(session_id, turn_id)
            .await?;
        if active_turn.status.is_terminal() {
            return Ok(());
        }
        if active_turn.status == ActiveTurnStatus::WaitingUserAction
            || (active_turn.status == ActiveTurnStatus::Cancelling
                && active_turn.current_action_id.is_some())
        {
            return self.event_log.cancel_waiting_user_action(active_turn).await;
        }
        let has_pending_action = active_turn.current_action_id.is_some();
        match self
            .access
            .cancel_active_turn(actor, session_id, active_turn.turn_id.clone())
            .await
        {
            Ok(
                CancelActiveTurnOutcome::Completed
                | CancelActiveTurnOutcome::Cancelled
                | CancelActiveTurnOutcome::ForceAborted,
            ) => {
                if has_pending_action {
                    self.event_log.cancel_waiting_user_action(active_turn).await
                } else {
                    Ok(())
                }
            }
            Ok(CancelActiveTurnOutcome::CleanupUnconfirmed { task_count }) => {
                Err(TalosError::public(
                    talos_error::ErrorCode::RunCleanupUnconfirmed,
                    format!("Run cancellation cleanup was not confirmed for {task_count} task(s)"),
                ))
            }
            Err(error) if error.is_code(talos_error::ErrorCode::ResourceNotFound) => {
                if has_pending_action {
                    return self.event_log.cancel_waiting_user_action(active_turn).await;
                }
                if active_turn.status == ActiveTurnStatus::Cancelling
                    && self
                        .event_log
                        .wait_for_turn_terminal(
                            session_id,
                            &active_turn.turn_id,
                            CANCEL_TERMINAL_WAIT,
                        )
                        .await?
                {
                    return Ok(());
                }
                tracing::warn!(
                    session_id = %session_id,
                    turn_id = %active_turn.turn_id,
                    missing_resource = %error.diagnostic_message(),
                    "finalizing orphaned active turn after interrupt"
                );
                self.finalize_orphaned_active_turn(session_id, &active_turn)
                    .await
            }
            Err(error) => {
                self.event_log
                    .restore_active_turn_on_cancel_error(&active_turn)
                    .await?;
                Err(error)
            }
        }
    }

    pub async fn interrupt_turn(
        &self,
        actor: &Actor,
        request: InterruptTurnRequest,
    ) -> ServiceResult<InterruptTurnResponse> {
        self.cancel_active_turn(actor, &request.session_id, &request.turn_id)
            .await?;
        Ok(InterruptTurnResponse { success: true })
    }
}
