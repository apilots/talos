//! Owner-scoped management of existing application-native callbacks.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Closed set of user-manageable jobs; internal Loop scans are not included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum BuiltinJobId {
    ProfileUsageRetention,
    DreamMemoryMaintenance,
}

impl BuiltinJobId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProfileUsageRetention => "profile-usage-retention",
            Self::DreamMemoryMaintenance => "dream-memory-maintenance",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum BuiltinJobAction {
    Pause,
    Resume,
    RunOnce,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum BuiltinJobOutcome {
    Succeeded,
    Failed { error: talos_error::ApiError },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
/// Existing callbacks operate on the execution instance's database, including
/// all related users/computers. `computer_id` names the actual callback host;
/// neither identifier is a data filter or proof of whole-instance ownership.
pub struct BuiltinJobInfo {
    pub id: BuiltinJobId,
    pub computer_id: String,
    pub execution_instance_id: String,
    pub name: String,
    pub schedule: String,
    pub timezone: String,
    pub paused: bool,
    pub running: bool,
    #[ts(type = "number | null")]
    pub next_due_at: Option<i64>,
    #[ts(type = "number | null")]
    pub last_finished_at: Option<i64>,
    pub last_outcome: Option<BuiltinJobOutcome>,
    pub available_actions: Vec<BuiltinJobAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinJobListRequest {
    pub computer_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinJobListResponse {
    pub data: Vec<BuiltinJobInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BuiltinJobControlRequest {
    pub computer_id: String,
    pub execution_instance_id: String,
    pub job_id: BuiltinJobId,
    pub action: BuiltinJobAction,
}
