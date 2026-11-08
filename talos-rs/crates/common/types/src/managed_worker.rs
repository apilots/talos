//! Account-scoped state for a Computer with a server-managed Worker.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_error::ApiError;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ManagedWorkerPhase {
    Provisioning,
    Starting,
    WaitingForRegistration,
    Ready,
    Sleeping,
    Stopped,
    Offline,
    Stopping,
    Resetting,
    Deleting,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ManagedWorkerOperation {
    Create,
    Start,
    Stop,
    Reset,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ManagedWorkerDiskQuota {
    Unsupported,
    Enforced,
}

/// Current lifecycle snapshot. Registration credentials and runtime resource IDs stay private.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ManagedWorkerInfo {
    pub instance_id: String,
    pub node_id: String,
    pub generation: u32,
    pub phase: ManagedWorkerPhase,
    pub operation: Option<ManagedWorkerOperation>,
    pub cpu_cores: u16,
    #[ts(type = "number")]
    pub memory_bytes: u64,
    pub disk_quota: ManagedWorkerDiskQuota,
    pub error: Option<ApiError>,
}

/// Deployment capability and the authenticated account's current instance, if any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ManagedWorkerResponse {
    pub enabled: bool,
    pub instance: Option<ManagedWorkerInfo>,
}

/// The caller must act on the instance generation it last observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct ManagedWorkerActionRequest {
    pub instance_id: String,
    pub expected_generation: u32,
}

/// Destructive action requiring explicit confirmation of permanent Home deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(rename_all = "camelCase")]
pub struct DeleteManagedWorkerRequest {
    pub instance_id: String,
    pub expected_generation: u32,
    pub confirm_data_loss: bool,
}
