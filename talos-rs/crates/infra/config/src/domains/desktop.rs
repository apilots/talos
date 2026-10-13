use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Product connection source, independent of the server deployment mode.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum DesktopConnectionMode {
    #[default]
    Cloud,
    Local,
}

/// Settings owned by the Desktop host. No credentials belong in this object.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopConfig {
    #[serde(default)]
    pub connection_mode: DesktopConnectionMode,
    /// Blocks duplicate creation after an unconfirmed enrollment or partial save.
    #[serde(default)]
    pub enrollment_pending: bool,
}
