use talos_types::{ModelRef, StopReason, TokenUsage};

/// Provides the aggregate usage values used to build a Profile response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileUsageSummary {
    pub usage: TokenUsage,
    pub usage_available: bool,
    pub usage_session_count: u64,
    pub usage_started_at_ms: Option<i64>,
}

/// Provides one finalized Profile day aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDailyUsageSummary {
    pub date: String,
    pub usage: TokenUsage,
    pub usage_available: bool,
}

/// Provides one finalized Profile model aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileModelUsageSummary {
    pub model_ref: ModelRef,
    pub turn_count: u64,
    pub usage: TokenUsage,
    pub usage_available: bool,
}

/// Carries one execution segment from the Turn service into Profile persistence.
///
/// The Turn service owns construction from a completed execution. Store
/// implementations use the request id only to suppress immediate duplicate
/// segments; durable replay and crash recovery are intentionally out of scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnUsageUpdate {
    pub user_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub request_id: String,
    pub model_ref: ModelRef,
    pub usage: TokenUsage,
    pub usage_available: bool,
    pub stop_reason: StopReason,
    pub timezone: String,
    pub completed_at_ms: i64,
    pub usage_date: String,
}

impl TurnUsageUpdate {
    pub fn is_final(&self) -> bool {
        matches!(
            self.stop_reason,
            StopReason::EndTurn | StopReason::ModelOutputLimit | StopReason::TransferUnresolved
        )
    }

    pub fn is_discarded(&self) -> bool {
        self.stop_reason == StopReason::Cancelled
    }
}
