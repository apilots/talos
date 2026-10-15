//! Canonical v1 Metrics HTTP DTOs. Talos consumes a mechanically synchronized copy.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct MetadataCounts {
    pub computers: u64,
    pub projects: u64,
    pub sessions: u64,
    pub files: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPoint {
    pub timestamp: i64,
    pub inbound_bytes_per_second: f64,
    pub outbound_bytes_per_second: f64,
    pub updates_per_second: f64,
    pub commit_p95_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDistribution {
    pub minimum_peers: u64,
    pub maximum_peers: Option<u64>,
    pub accounts: u64,
    pub peers: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublicMetrics {
    pub api_version: u32,
    pub observed_at: i64,
    pub last_flushed_at: Option<i64>,
    pub complete: bool,
    pub stopping: bool,
    pub range_seconds: u64,
    pub bucket_seconds: u64,
    pub online_accounts: u64,
    pub online_peers: u64,
    pub connections: u64,
    pub active_peers: u64,
    pub document_rooms: u64,
    pub updates_per_second: f64,
    pub inbound_bytes_per_second: f64,
    pub outbound_bytes_per_second: f64,
    pub commit_p95_ms: Option<f64>,
    pub failure_rate: Option<f64>,
    pub metadata: MetadataCounts,
    pub distribution: Vec<ConnectionDistribution>,
    pub history: Vec<HistoryPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerMetrics {
    pub peer_id: String,
    /// Unexpired, self-published presence; join only to the authorized Computer directory.
    pub computer_ids: Vec<String>,
    pub connections: u64,
    pub document_rooms: u64,
    pub last_activity_at: Option<i64>,
    pub inbound_bytes_per_second: f64,
    pub outbound_bytes_per_second: f64,
    pub updates_per_second: f64,
    pub commit_p95_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountMetrics {
    pub api_version: u32,
    pub observed_at: i64,
    pub last_flushed_at: Option<i64>,
    pub complete: bool,
    pub account_id: String,
    pub metadata: MetadataCounts,
    pub peers: Vec<PeerMetrics>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MetricsContract {
    pub public: PublicMetrics,
    pub account: AccountMetrics,
}
