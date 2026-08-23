use talos_types::{ModelRef, ModelUsageKind, StopReason, TokenUsage};

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

/// Carries one completed non-Turn model call into Profile persistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCallUsageUpdate {
    /// Profile owner charged for the model call.
    pub user_id: String,
    /// Optional Session that originated the call.
    pub session_id: Option<String>,
    /// Idempotency identity for this completed Provider call.
    pub request_id: String,
    /// Workload classification persisted as `usage_kind`.
    pub kind: ModelUsageKind,
    /// Provider and model that produced the usage.
    pub model_ref: ModelRef,
    /// Provider-reported Token usage.
    pub usage: TokenUsage,
    /// Whether the Provider returned any measurable usage value.
    pub usage_available: bool,
    /// Timezone used to derive `usage_date`.
    pub timezone: String,
    /// Provider call completion time as Unix milliseconds.
    pub completed_at_ms: i64,
    /// Local calendar date used by Profile daily aggregation.
    pub usage_date: String,
}

/// Selects the lifecycle used to persist a Profile model-usage update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileUsageUpdate {
    /// Turn lifecycle update with waiting, finalization, and discard semantics.
    Turn(TurnUsageUpdate),
    /// One already-completed non-Turn model call.
    ModelCall(ModelCallUsageUpdate),
}
