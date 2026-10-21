//! Closed Desktop website-host contract; no arbitrary CDP, scripts or endpoints.
use crate::{BrowserAction, BrowserTargetKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHostRequest {
    pub generation: String,
    #[ts(type = "number")]
    pub epoch: u64,
    pub user_id: String,
    pub target: BrowserTargetKind,
    pub operation: BrowserHostOperation,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[serde(deny_unknown_fields)]
pub enum BrowserHostOperation {
    Probe,
    Execute {
        action: BrowserAction,
        target_id: Option<String>,
        element: Option<BrowserHostElement>,
        upload: Option<BrowserHostUpload>,
    },
    AcknowledgeDownloads {
        files: Vec<BrowserDownloadReceipt>,
    },
    Connect,
    Disconnect,
    Show,
    ReleaseInputs,
    Shutdown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHostElement {
    pub backend_node_id: u32,
    pub tag: String,
    pub attributes: HashMap<String, String>,
    pub target_id: String,
    pub loader_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHostPage {
    pub target_id: String,
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub tabs: Vec<BrowserHostTab>,
    pub text: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHostTab {
    pub tab_id: String,
    pub title: String,
    pub url: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[serde(deny_unknown_fields)]
pub enum BrowserHostResponse {
    Readiness {
        connected: bool,
    },
    Done,
    Observation {
        page: BrowserHostPage,
        loader_id: Option<String>,
        elements: Vec<BrowserHostElement>,
        screenshot: Option<String>,
        downloads: Vec<BrowserHostFile>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHostCancel {
    pub request_id: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHostUpload {
    pub filename: String,
    pub data_base64: String,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHostFile {
    pub filename: String,
    pub data_base64: String,
}

/// Protected Desktop IPC last mile for the separate Agent media stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct DesktopMediaFrame {
    pub stream_id: String,
    pub data_base64: Option<String>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopStreamOpen {
    pub stream_id: String,
    pub ticket: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopStreamSend {
    pub stream_id: String,
    pub data_base64: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopStreamClose {
    pub stream_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDownloadReceipt {
    pub filename: String,
    pub sha256: String,
}

impl std::fmt::Debug for DesktopStreamOpen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopStreamOpen")
            .field("stream_id", &self.stream_id)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for BrowserHostUpload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserHostUpload")
            .field("filename", &self.filename)
            .field("encoded_bytes", &self.data_base64.len())
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for BrowserHostFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserHostFile")
            .field("filename", &self.filename)
            .field("encoded_bytes", &self.data_base64.len())
            .finish_non_exhaustive()
    }
}
