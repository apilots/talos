use crate::CoordinatorImpl;
use std::time::Duration;
use talos_error::{TalosError, TalosResult};

impl CoordinatorImpl {
    pub async fn drain_background_tasks(&self, timeout: Duration) -> TalosResult<()> {
        self.drain_memory_flushes(timeout).await
    }

    pub async fn pending_memory_flushes(&self) -> usize {
        let agents = self
            .agents
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        agents
            .iter()
            .map(|agent| agent.pending_memory_flushes())
            .sum()
    }

    pub async fn drain_memory_flushes(&self, timeout: Duration) -> TalosResult<()> {
        let deadline = std::time::Instant::now() + timeout;
        let agents = self
            .agents
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();

        for agent in agents {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .ok_or_else(|| {
                    TalosError::InternalError(
                        "memory flush drain timed out before all agents were drained".to_string(),
                    )
                })?;
            agent.drain_memory_flushes(remaining).await?;
        }

        Ok(())
    }
}
