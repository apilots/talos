//! Authenticated account observations. Presence is self-reported, not execution authority.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SyncMetricsResponse {
    /// Safe origin of the configured synchronization service; no credentials or query.
    pub endpoint: String,
    /// Current host's peer only when this management host is a Control Plane.
    pub control_plane_peer_id: Option<String>,
    #[ts(type = "number")]
    pub observed_at: i64,
    #[ts(type = "number | null")]
    pub last_flushed_at: Option<i64>,
    pub complete: bool,
    pub metadata: SyncMetadataCounts,
    pub peers: Vec<SyncPeerMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SyncMetadataCounts {
    #[ts(type = "number")]
    pub computers: u64,
    #[ts(type = "number")]
    pub projects: u64,
    #[ts(type = "number")]
    pub sessions: u64,
    #[ts(type = "number")]
    pub files: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SyncPeerMetrics {
    pub peer_id: String,
    /// Match only against the caller's authorized Computer directory.
    pub computer_ids: Vec<String>,
    #[ts(type = "number")]
    pub connections: u64,
    #[ts(type = "number")]
    pub document_rooms: u64,
    #[ts(type = "number | null")]
    pub last_activity_at: Option<i64>,
    /// Sync service receipt of the last solicited heartbeat pong.
    #[ts(type = "number | null")]
    pub last_heartbeat_at: Option<i64>,
    pub inbound_bytes_per_second: f64,
    pub outbound_bytes_per_second: f64,
    pub updates_per_second: f64,
    pub commit_p95_ms: Option<f64>,
}
