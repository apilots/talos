use crate::CoordinatorImpl;
use std::time::{Duration, Instant};
use talos_error::{TalosError, TalosResult};
use tracing::{info, warn};

impl CoordinatorImpl {
    /// Drain Coordinator-owned background work across all registered Agents.
    ///
    /// This lifecycle hook coordinates Harness drains only. It must not inspect
    /// Service store records or runtime-private task handles directly.
    pub async fn drain_background_tasks(&self, timeout: Duration) -> TalosResult<()> {
        let started_at = Instant::now();
        info!(
            timeout_ms = timeout.as_millis() as u64,
            "background task drain started"
        );
        let active_turn_deadline = Instant::now() + timeout;
        if let Err(err) = self.drain_active_turns(active_turn_deadline).await {
            warn!(error = %err, "active turn drain failed during background task drain");
            return Err(err);
        }
        info!(
            elapsed_ms = started_at.elapsed().as_millis() as u64,
            "background task drain completed"
        );
        Ok(())
    }

    /// Wait for in-memory active Agent turns to finish before the deadline.
    ///
    /// Active turn ownership remains in each AgentHarness; Coordinator only
    /// fans out the drain request across the registry.
    pub async fn drain_active_turns(&self, deadline: std::time::Instant) -> TalosResult<()> {
        let agents = self.registry().list().await;

        for registration in agents {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .ok_or_else(|| {
                    TalosError::internal(
                        "drain_active_turns timed out before all agents were drained".to_string(),
                    )
                })?;
            registration.harness.drain_active_turns(remaining).await?;
        }

        Ok(())
    }
}
