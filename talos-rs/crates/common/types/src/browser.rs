//! Browser-specific host capabilities and environment operations.
//!
//! A local website presentation is not a network display permission. Node routing
//! and caller identity are supplied by the existing environment boundary.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const BUILTIN_BROWSER_PLUGIN_ID: &str = "builtin_browser";
pub const BUILTIN_BROWSER_SERVER_NAME: &str = "talos_browser";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum BrowserHostKind {
    Docker,
    Desktop,
    Native,
    Server,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum BrowserTargetKind {
    Managed,
    Connected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum BrowserReadiness {
    Stopped,
    Starting,
    Ready,
    NeedsConnection,
    Unavailable { error: talos_error::ApiError },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum BrowserDisplayUnavailableReason {
    NativeHost,
    NotBundled,
    HostPolicy,
    UnsupportedVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum BrowserDisplayCapability {
    DockerDesktop {
        target_id: String,
    },
    Unavailable {
        reason: BrowserDisplayUnavailableReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum BrowserLocalOpenCapability {
    DesktopOwned,
    ExternalUserWindow,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTarget {
    pub target_id: String,
    pub display_name: String,
    pub kind: BrowserTargetKind,
    pub readiness: BrowserReadiness,
    pub local_open: BrowserLocalOpenCapability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum BrowserSuspensionReason {
    UserPaused,
    ViewerDisconnected,
    WorkerRestarted,
    TakeoverFailed,
    PluginDisabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum BrowserControlOwner {
    Idle,
    Agent { session_id: String, turn_id: String },
    HandingOver { controller_id: String },
    Human { controller_id: String },
    Suspended { reason: BrowserSuspensionReason },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BrowserControlState {
    pub generation: String,
    #[ts(type = "number")]
    pub revision: u64,
    #[ts(type = "number")]
    pub epoch: u64,
    pub owner: BrowserControlOwner,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSnapshot {
    pub enabled: bool,
    pub host: BrowserHostKind,
    pub selected_target_id: Option<String>,
    pub targets: Vec<BrowserTarget>,
    pub display: BrowserDisplayCapability,
    pub control: BrowserControlState,
}

/// Closed environment operations; neither model arguments nor arbitrary CDP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[serde(deny_unknown_fields)]
pub enum BrowserOperation {
    Read,
    SelectTarget {
        target_id: String,
        #[ts(type = "number")]
        expected_revision: u64,
    },
    Connect {
        #[ts(type = "number")]
        expected_revision: u64,
    },
    Disconnect {
        #[ts(type = "number")]
        expected_revision: u64,
    },
    Pause {
        #[ts(type = "number")]
        expected_revision: u64,
    },
    Resume {
        #[ts(type = "number")]
        expected_revision: u64,
    },
    TakeControl {
        #[ts(type = "number")]
        expected_revision: u64,
    },
    ReleaseControl {
        controller_id: String,
        #[ts(type = "number")]
        expected_revision: u64,
    },
    OpenLocal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub node_id: Option<String>,
    pub operation: BrowserOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum BrowserInputMode {
    ReadOnly,
    Control,
}

/// Secret single-use ticket. Never include it in URLs, logs or model input.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct DesktopViewGrant {
    pub ticket: String,
    pub stream_id: String,
    pub viewer_id: String,
    pub generation: String,
    pub input_mode: BrowserInputMode,
    pub controller_id: Option<String>,
    #[ts(type = "number | null")]
    pub input_epoch: Option<u64>,
    #[ts(type = "number")]
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopOpenRequest {
    pub node_id: String,
    pub target_id: String,
    pub viewer_id: String,
    pub generation: String,
    pub input_mode: BrowserInputMode,
    pub controller_id: Option<String>,
    #[ts(type = "number | null")]
    pub input_epoch: Option<u64>,
}

/// Immutable Worker-side input authority. Never update an existing RFB stream's epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopStreamBinding {
    pub stream_id: String,
    pub target_id: String,
    pub viewer_id: String,
    pub generation: String,
    pub input_mode: BrowserInputMode,
    pub controller_id: Option<String>,
    pub input_epoch: Option<u64>,
}

/// Private control-plane-to-Worker setup; the nonce only correlates the separate
/// outbound media RPC at the Worker's preconfigured trusted ingress.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerDesktopOpen {
    pub binding: DesktopStreamBinding,
    pub nonce: String,
}

impl std::fmt::Debug for DesktopViewGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopViewGrant")
            .field("stream_id", &self.stream_id)
            .field("input_mode", &self.input_mode)
            .finish_non_exhaustive()
    }
}
