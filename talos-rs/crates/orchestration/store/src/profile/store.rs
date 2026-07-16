use async_trait::async_trait;
use sqlx::SqlitePool;
use talos_error::TalosResult;

use super::{
    ProfileDailyUsageSummary, ProfileModelUsageSummary, ProfileUsageSummary, TurnUsageUpdate,
};

/// Reads and records Profile usage without exposing database records.
///
/// The Profile service uses this interface for user-scoped aggregates and the
/// Turn service records completed execution segments. Implementations own SQL
/// and persistence validation; callers do not receive transactions or rows.
#[async_trait]
pub trait ProfileStore: Send + Sync {
    async fn record_turn_usage(&self, update: TurnUsageUpdate) -> TalosResult<()>;

    /// Removes unfinished usage when a logical Turn fails before finalization.
    async fn discard_turn_usage(&self, session_id: &str, turn_id: &str) -> TalosResult<()>;

    /// Deletes up to `limit` finalized usage rows completed before `completed_before_ms`.
    async fn delete_finalized_before(
        &self,
        completed_before_ms: i64,
        limit: usize,
    ) -> TalosResult<u64>;

    async fn usage_summary(&self, user_id: &str) -> TalosResult<ProfileUsageSummary>;

    async fn daily_usage(
        &self,
        user_id: &str,
        start_date: &str,
        end_date: &str,
    ) -> TalosResult<Vec<ProfileDailyUsageSummary>>;

    async fn model_usage(
        &self,
        user_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<ProfileModelUsageSummary>>;
}

/// SQLite-backed Profile usage repository.
pub struct DatabaseProfileStore {
    pub(super) pool: SqlitePool,
}

impl DatabaseProfileStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ProfileStore for DatabaseProfileStore {
    async fn record_turn_usage(&self, update: TurnUsageUpdate) -> TalosResult<()> {
        super::write::record_turn_usage(&self.pool, update).await
    }

    async fn discard_turn_usage(&self, session_id: &str, turn_id: &str) -> TalosResult<()> {
        super::write::discard_turn_usage(&self.pool, session_id, turn_id).await
    }

    async fn delete_finalized_before(
        &self,
        completed_before_ms: i64,
        limit: usize,
    ) -> TalosResult<u64> {
        super::write::delete_finalized_before(&self.pool, completed_before_ms, limit).await
    }

    async fn usage_summary(&self, user_id: &str) -> TalosResult<ProfileUsageSummary> {
        super::query::usage_summary(&self.pool, user_id).await
    }

    async fn daily_usage(
        &self,
        user_id: &str,
        start_date: &str,
        end_date: &str,
    ) -> TalosResult<Vec<ProfileDailyUsageSummary>> {
        super::query::daily_usage(&self.pool, user_id, start_date, end_date).await
    }

    async fn model_usage(
        &self,
        user_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<ProfileModelUsageSummary>> {
        super::query::model_usage(&self.pool, user_id, limit).await
    }
}
