use std::sync::Arc;

use chrono::{DateTime, Utc};
use talos_config::ProfileUsageRetentionConfig;
use talos_error::TalosResult;
use talos_store::profile::ProfileStore;
use talos_store::session::{ComputerDataScope, MaintenanceDataScope, UnassignedComputerData};

/// Removes expired finalized Profile usage rows with bounded database work.
#[derive(Clone)]
pub struct ProfileUsageRetention {
    profiles: Arc<dyn ProfileStore>,
    config: ProfileUsageRetentionConfig,
}

impl ProfileUsageRetention {
    /// Creates the Profile-owned retention operation from one store and policy.
    pub fn new(profiles: Arc<dyn ProfileStore>, config: ProfileUsageRetentionConfig) -> Self {
        Self { profiles, config }
    }

    /// Runs the configured retention policy once and returns the deleted row count.
    pub async fn run(&self) -> TalosResult<u64> {
        self.run_at(Utc::now(), MaintenanceDataScope::Application)
            .await
    }

    /// Retain only usage attributable to Sessions on the verified Computer.
    pub async fn run_for_computer(&self, computer: &ComputerDataScope) -> TalosResult<u64> {
        self.run_at(Utc::now(), MaintenanceDataScope::Computer(computer))
            .await
    }

    pub async fn run_unassigned(&self, unassigned: &UnassignedComputerData) -> TalosResult<u64> {
        self.run_at(Utc::now(), MaintenanceDataScope::Unassigned(unassigned))
            .await
    }

    pub(super) async fn run_at(
        &self,
        now: DateTime<Utc>,
        scope: MaintenanceDataScope<'_>,
    ) -> TalosResult<u64> {
        let retention_ms = i64::from(self.config.retention_days) * 24 * 60 * 60 * 1_000;
        let cutoff = now.timestamp_millis().saturating_sub(retention_ms);
        let mut deleted = 0;

        for _ in 0..self.config.max_batches {
            let batch = self
                .profiles
                .delete_finalized_before(cutoff, self.config.batch_size, scope)
                .await?;
            deleted += batch;
            if batch < self.config.batch_size as u64 {
                break;
            }
        }

        Ok(deleted)
    }
}
