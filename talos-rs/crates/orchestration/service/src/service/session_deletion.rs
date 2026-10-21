//! Session deletion requires confirmed idle execution, with a bounded preflight.
use super::{Actor, ServiceResult, TalosService};
use talos_error::{ErrorCode, TalosError};

const UNCONFIRMED_DELETION_MESSAGE: &str =
    "Session deletion could not confirm idle execution. Retry when its Computer is available.";
const DELETION_PREFLIGHT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

impl TalosService {
    /// Reject known activity locally, then bound Worker verification and cleanup.
    /// An unavailable Worker never proves idleness. Only a successful preflight
    /// permits the original Store deletion and transactional Sync capture.
    pub async fn delete_session(&self, actor: &Actor, session_id: &str) -> ServiceResult<bool> {
        let session = self.access.load_session(actor, session_id).await?;
        self.ensure_deletion_idle(session_id).await?;
        let (session, _guard) = tokio::time::timeout(DELETION_PREFLIGHT_TIMEOUT, async {
            let guard = self
                .execution_gate
                .for_node(&session.execution_node_id)
                .write_owned()
                .await;
            self.ensure_deletion_idle(session_id).await?;
            self.connect_selected_computer(actor, &session.execution_node_id)
                .await?;
            self.subagents.ensure_tree_inactive(session_id).await?;
            let session = self.access.prepare_session_deletion(session).await?;
            Ok::<_, TalosError>((session, guard))
        })
        .await
        .map_err(|_| TalosError::new(ErrorCode::ServiceUnavailable, UNCONFIRMED_DELETION_MESSAGE))?
        .map_err(|error| match error.error_code() {
            ErrorCode::ServiceUnavailable | ErrorCode::Timeout => TalosError::from_source(
                ErrorCode::ServiceUnavailable,
                UNCONFIRMED_DELETION_MESSAGE,
                error,
            ),
            _ => error,
        })?;
        // Do not time out the durable commit: a timed-out preflight cannot delete data.
        let deleted = self.sessions.delete_session(session).await?;
        if deleted {
            self.message_queue.clear_session(session_id)?;
        }
        Ok(deleted)
    }

    async fn ensure_deletion_idle(&self, session_id: &str) -> ServiceResult<()> {
        if self.events.has_live_turn(session_id).await? {
            return Err(TalosError::new(
                ErrorCode::ActiveTurnRunning,
                "Session has a nonterminal Turn and cannot be deleted.",
            ));
        }
        self.subagents
            .ensure_tree_locally_inactive(session_id)
            .await
    }
}
