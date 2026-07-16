use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{ModelRef, TokenUsage};

/// Requests the authenticated user's Profile usage summary.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ProfileReadRequest {}

/// Returns the authenticated user's current resource counts and historical usage.
///
/// Gateway, SDK, and UI boundaries share this DTO. It does not expose database
/// records or support querying another user's data.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileReadResponse {
    #[ts(type = "number")]
    pub session_count: u64,
    #[ts(type = "number")]
    pub message_count: u64,
    #[ts(type = "number")]
    pub project_count: u64,
    pub usage: TokenUsage,
    pub usage_available: bool,
    #[ts(type = "number")]
    pub usage_session_count: u64,
    #[ts(type = "number | null")]
    pub usage_started_at: Option<i64>,
    pub as_of_date: String,
    pub daily_usage: Vec<ProfileDailyUsage>,
    pub model_usage: Vec<ProfileModelUsage>,
}

/// Represents finalized usage attributed to one calendar day.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDailyUsage {
    pub date: String,
    pub usage: TokenUsage,
    pub usage_available: bool,
}

/// Represents finalized usage grouped by its recorded model.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileModelUsage {
    pub model_ref: ModelRef,
    #[ts(type = "number")]
    pub turn_count: u64,
    pub usage: TokenUsage,
    pub usage_available: bool,
}
