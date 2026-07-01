use crate::CoordinatorImpl;
use std::time::{Duration, Instant};
use talos_error::{TalosError, TalosResult};
use tracing::{info, warn};

impl CoordinatorImpl {
    pub async fn drain_background_tasks(&self, timeout: Duration) -> TalosResult<()> {
        let started_at = Instant::now();
        let pending_memory_flushes = self.pending_memory_flushes().await;
        info!(
            pending_memory_flushes,
            timeout_ms = timeout.as_millis() as u64,
            "background task drain started"
        );
        let active_turn_deadline = Instant::now() + timeout;

        let (active_turn_result, memory_flush_result) = tokio::join!(
            self.drain_active_turns(active_turn_deadline),
            self.drain_memory_flushes(timeout)
        );

        if let Err(err) = &active_turn_result {
            warn!(error = %err, "active turn drain failed during background task drain");
        }
        if let Err(err) = &memory_flush_result {
            warn!(error = %err, "memory flush drain failed during background task drain");
        }
        memory_flush_result?;
        active_turn_result?;
        info!(
            elapsed_ms = started_at.elapsed().as_millis() as u64,
            "background task drain completed"
        );
        Ok(())
    }

    pub async fn drain_active_turns(&self, deadline: std::time::Instant) -> TalosResult<()> {
        let agents = self.registry().list().await;

        for registration in agents {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .ok_or_else(|| {
                    TalosError::InternalError(
                        "drain_active_turns timed out before all agents were drained".to_string(),
                    )
                })?;
            registration.harness.drain_active_turns(remaining).await?;
        }

        Ok(())
    }

    pub async fn pending_memory_flushes(&self) -> usize {
        let agents = self.registry().list().await;
        agents
            .iter()
            .map(|registration| registration.harness.pending_memory_flushes())
            .sum()
    }

    pub async fn drain_memory_flushes(&self, timeout: Duration) -> TalosResult<()> {
        let started_at = Instant::now();
        let pending_memory_flushes = self.pending_memory_flushes().await;
        info!(
            pending_memory_flushes,
            timeout_ms = timeout.as_millis() as u64,
            "coordinator memory flush drain started"
        );
        let deadline = Instant::now() + timeout;
        let agents = self.registry().list().await;

        for registration in agents {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .ok_or_else(|| {
                    TalosError::InternalError(
                        "memory flush drain timed out before all agents were drained".to_string(),
                    )
                })?;
            registration.harness.drain_memory_flushes(remaining).await?;
        }

        info!(
            elapsed_ms = started_at.elapsed().as_millis() as u64,
            "coordinator memory flush drain completed"
        );
        Ok(())
    }
}
