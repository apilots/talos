//! Canonical public Computer identity and availability. Secrets remain in node control.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum NodeMode {
    Embedded,
    Remote,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum NodeStatus {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum NodeHealth {
    Unknown,
    Healthy,
    Degraded,
}

/// Supported Worker operating systems. Hardware is self-reported presentation metadata.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum NodeOperatingSystem {
    Linux,
    MacOs,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct NodeNetworkInterface {
    pub name: String,
    pub mac_address: String,
}

/// Static environment visible to the Worker; unavailable optional information stays null.
/// No credentials, IP addresses, device serial numbers or execution state belong here.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct NodeSystemInfo {
    pub os: NodeOperatingSystem,
    pub os_version: Option<String>,
    pub architecture: String,
    pub hostname: Option<String>,
    pub cpu_model: Option<String>,
    pub logical_cpu_count: u32,
    #[ts(type = "number")]
    pub memory_bytes: u64,
    pub board_model: Option<String>,
    pub network_interfaces: Vec<NodeNetworkInterface>,
    pub graphics: Vec<String>,
    pub worker_version: String,
}

/// Public node state; credential and lease hashes stay inside the Store.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NodeInfo {
    pub node_id: String,
    pub display_name: String,
    pub mode: NodeMode,
    pub endpoint: Option<String>,
    pub status: NodeStatus,
    pub health: NodeHealth,
    #[ts(type = "number | null")]
    pub lease_expires_at: Option<i64>,
    #[ts(type = "number | null")]
    pub last_heartbeat_at: Option<i64>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
    pub owner_user_id: Option<String>,
    pub available: bool,
    #[ts(type = "number | null")]
    pub credential_expires_at: Option<i64>,
    pub system_info: Option<NodeSystemInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NodePage {
    pub data: Vec<NodeInfo>,
    pub next_cursor: Option<String>,
}

/// A one-time management response that must never be logged or re-readable.
#[derive(Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct NodeCredential {
    pub node_id: String,
    pub token: String,
    #[ts(type = "number")]
    pub expires_at: i64,
}

#[derive(Clone, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CreatedNode {
    pub node: NodeInfo,
    pub credential: NodeCredential,
}

/// Creates a node with a control-plane-generated, immutable UUID.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CreateNodeRequest {
    pub display_name: String,
    /// None selects an outbound connection; Some requires a nonempty HTTPS origin.
    #[ts(optional = nullable)]
    pub endpoint: Option<String>,
    #[ts(optional = nullable)]
    pub owner_user_id: Option<String>,
    #[ts(type = "number")]
    pub credential_expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct GetNodeRequest {
    pub node_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ListNodesRequest {
    #[ts(optional = nullable)]
    pub cursor: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct UpdateNodeRequest {
    pub node_id: String,
    pub display_name: String,
    /// None selects an outbound connection; Some requires a nonempty HTTPS origin.
    #[ts(optional = nullable)]
    pub endpoint: Option<String>,
    pub status: NodeStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RotateCredentialRequest {
    pub node_id: String,
    #[ts(type = "number")]
    pub expires_at: i64,
}

/// Selectable Computers for the caller, including the existing shared embedded node.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ComputerListRequest {
    #[ts(optional = nullable)]
    pub query: Option<String>,
    #[ts(optional = nullable)]
    pub cursor: Option<String>,
    #[ts(optional = nullable)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ComputerListResponse {
    pub data: Vec<NodeInfo>,
    pub next_cursor: Option<String>,
    #[ts(type = "number")]
    pub total: u64,
}

/// Only the display name changes; endpoint, lease and status remain intact.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct RenameComputerRequest {
    pub node_id: String,
    pub display_name: String,
}

/// Disabling a Computer revokes execution admission, without shutting down its host.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SetComputerStatusRequest {
    pub node_id: String,
    pub status: NodeStatus,
}

/// Remove an owned registration, retaining history and Workspace files.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DeleteComputerRequest {
    pub node_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DeleteComputerResponse {
    pub node_id: String,
}
