//! Safe Desktop host APIs. Node credentials never cross this boundary.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DesktopWorkerIdentityResponse {
    pub node_id: Option<String>,
    pub control_plane_endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct DesktopWorkerEnrollParams {
    /// Explicit UUID for credential recovery. Omitted/null enrolls or reuses saved identity.
    #[ts(optional = nullable)]
    pub node_id: Option<String>,
}
