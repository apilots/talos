use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Reason a run entered the context compaction maintenance stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ContextCompactionTrigger {
    UserRequested,
    ThresholdExceeded,
    HardThresholdExceeded,
    BudgetExceeded,
    HistorySectionsDropped,
    WorkingContextExceeded,
}

/// Current stage of a context compaction timeline item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ContextCompactionStage {
    Preparing,
    Summarizing,
    RebuildingPrompt,
    Trimming,
    Completed,
    Failed,
}

/// Terminal result of a context compaction maintenance item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ContextCompactionOutcome {
    Applied,
    NoChanges,
    RejectedMissingSummary,
    TrimmedFallbackApplied,
    FailedContextStillTooLarge,
    FailedProviderError,
    FailedInternalError,
}

/// Public metrics for a context compaction maintenance item.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ContextCompactionMetrics {
    #[serde(default)]
    #[ts(type = "number | null")]
    pub covers_from_seq: Option<u64>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub covers_until_seq: Option<u64>,
    pub pre_compaction_total_tokens: u32,
    pub post_compaction_total_tokens: u32,
    pub compaction_input_tokens: u32,
    pub compaction_output_tokens: u32,
    pub trimmed_context_unit_count: u32,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub raw_tail_preferred_count: Option<u32>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub raw_tail_backfilled_count: Option<u32>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub working_context_input_messages: Option<u32>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub working_context_compacted_messages: Option<u32>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub working_context_preserved_recent_exchanges: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct EventUnsubscribeRequest {
    pub subscription_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct EventUnsubscribeResponse {
    pub subscription_id: String,
    pub unsubscribed: bool,
}
