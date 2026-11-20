use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use talos_error::{TalosError, TalosResult};

use super::{
    ProfileDailyUsageSummary, ProfileModelUsageSummary, ProfileStore, ProfileUsageSummary,
    ProfileUsageUpdate,
};

/// In-memory Profile Store used by Agent and Service tests.
#[derive(Default)]
pub struct MockProfileStore {
    updates: Mutex<Vec<ProfileUsageUpdate>>,
    fail_record_usage: AtomicBool,
}

impl MockProfileStore {
    /// Returns all usage updates recorded by this test Store.
    pub fn updates(&self) -> Vec<ProfileUsageUpdate> {
        self.updates
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Enables or disables deterministic usage-recording failure injection.
    pub fn set_record_usage_failure(&self, fail: bool) {
        self.fail_record_usage.store(fail, Ordering::Relaxed);
    }
}

#[async_trait]
impl ProfileStore for MockProfileStore {
    async fn record_usage(&self, update: ProfileUsageUpdate) -> TalosResult<()> {
        if self.fail_record_usage.load(Ordering::Relaxed) {
            return Err(TalosError::internal("injected profile usage failure"));
        }
        self.updates
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(update);
        Ok(())
    }

    async fn discard_turn_usage(&self, _session_id: &str, _turn_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn delete_finalized_before(
        &self,
        _completed_before_ms: i64,
        _limit: usize,
    ) -> TalosResult<u64> {
        Ok(0)
    }

    async fn usage_summary(&self, _user_id: &str) -> TalosResult<ProfileUsageSummary> {
        Ok(ProfileUsageSummary {
            usage: Default::default(),
            usage_available: false,
            usage_session_count: 0,
            usage_started_at_ms: None,
        })
    }

    async fn daily_usage(
        &self,
        _user_id: &str,
        _start_date: &str,
        _end_date: &str,
    ) -> TalosResult<Vec<ProfileDailyUsageSummary>> {
        Ok(Vec::new())
    }

    async fn model_usage(
        &self,
        _user_id: &str,
        _limit: usize,
    ) -> TalosResult<Vec<ProfileModelUsageSummary>> {
        Ok(Vec::new())
    }
}
