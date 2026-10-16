//! Product synchronization controls and authenticated host observations.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum SyncConnectionState {
    Disabled,
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SyncEnableRequest {
    /// Explicitly claim this trusted local host's anonymous history. Omission means false.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub claim_local_history: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SyncStatusResponse {
    /// Null follows verified login; false persists an explicit opt-out.
    pub configured_enabled: Option<bool>,
    pub connection: SyncConnectionState,
    /// Current management peer; scopes local pending/error observations in topology.
    pub peer_id: Option<String>,
    #[ts(type = "number")]
    pub pending_changes: u64,
    #[ts(type = "number | null")]
    pub last_acknowledged_at: Option<i64>,
    pub last_error: Option<talos_error::ApiError>,
    /// This management host's accessible Computer UUID, or embedded before enrollment.
    /// Null means the signed-in account does not own this shared host's private Worker.
    pub local_computer_id: Option<String>,
    /// True only for a trusted local host; cloud users cannot claim shared anonymous data.
    pub can_claim_local_history: bool,
}
