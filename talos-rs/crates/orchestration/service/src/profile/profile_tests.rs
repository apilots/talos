use std::sync::Arc;

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use talos_error::TalosResult;
use talos_store::profile::{
    ProfileDailyUsageSummary, ProfileModelUsageSummary, ProfileStore, ProfileUsageSummary,
    TurnUsageUpdate,
};
use talos_store::project::MockProjectStore;
use talos_store::session::MockSessionStore;
use talos_types::TokenUsage;

use super::ProfileService;
use crate::Actor;

struct TestProfileStore;

#[async_trait]
impl ProfileStore for TestProfileStore {
    async fn record_turn_usage(&self, _update: TurnUsageUpdate) -> TalosResult<()> {
        Ok(())
    }

    async fn discard_turn_usage(&self, _session_id: &str, _turn_id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn usage_summary(&self, _user_id: &str) -> TalosResult<ProfileUsageSummary> {
        Ok(ProfileUsageSummary {
            usage: TokenUsage::default(),
            usage_available: false,
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

#[tokio::test]
async fn profile_window_uses_the_configured_timezone() {
    let service = ProfileService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        Arc::new(TestProfileStore),
        "Asia/Shanghai".to_string(),
    );
    let now = Utc.with_ymd_and_hms(2026, 7, 15, 16, 30, 0).unwrap();

    let profile = service
        .read_profile_at(&Actor::regular("user-1"), now)
        .await
        .unwrap();

    assert_eq!(profile.as_of_date, "2026-07-16");
    assert!(profile.daily_usage.is_empty());
}
