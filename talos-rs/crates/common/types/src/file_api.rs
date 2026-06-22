use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ExpectedFileVersion {
    MustNotExist,
    Match { version_token: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct DocUpdateRequest {
    pub content: String,
    pub base_version: ExpectedFileVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct MemoryFileListRequest {
    #[serde(default)]
    #[ts(optional)]
    pub limit: Option<usize>,
    #[serde(default)]
    #[ts(optional)]
    pub before_date: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub date_from: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub date_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemoryFileListResponse {
    pub data: Vec<MemoryFileSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemoryFileReadRequest {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemoryFileReadResponse {
    pub file: MemoryFileDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemoryFileWriteRequest {
    pub path: PathBuf,
    pub content: String,
    #[serde(default)]
    #[ts(optional)]
    pub expected_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemoryFileWriteResponse {
    pub file: MemoryFileDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemorySearchRequest {
    pub query: String,
    #[serde(default)]
    #[ts(optional)]
    pub limit: Option<usize>,
    #[serde(default)]
    #[ts(optional)]
    pub date_from: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub date_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemorySearchResponse {
    #[serde(default)]
    pub results: Vec<MemorySearchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemorySearchResult {
    pub path: String,
    pub snippet: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemoryFileSummary {
    pub date: String,
    pub path: String,
    pub section_count: usize,
    #[ts(type = "number | null")]
    pub updated_at: Option<i64>,
    pub version_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemoryFileDetail {
    pub date: String,
    pub path: String,
    pub section_count: usize,
    pub content: String,
    #[ts(type = "number | null")]
    pub updated_at: Option<i64>,
    pub version_token: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceDocKind {
    Soul,
    Identity,
    User,
    Heartbeat,
    Memory,
}

impl WorkspaceDocKind {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Soul => "SOUL.md",
            Self::Identity => "IDENTITY.md",
            Self::User => "USER.md",
            Self::Heartbeat => "HEARTBEAT.md",
            Self::Memory => "MEMORY.md",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct WorkspaceDocListRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct WorkspaceDocListResponse {
    pub data: Vec<WorkspaceDocSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct WorkspaceDocReadRequest {
    pub kind: WorkspaceDocKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct WorkspaceDocReadResponse {
    pub doc: WorkspaceDocInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct WorkspaceDocWriteRequest {
    pub kind: WorkspaceDocKind,
    pub content: String,
    #[serde(default)]
    #[ts(optional)]
    pub expected_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct WorkspaceDocWriteResponse {
    pub doc: WorkspaceDocInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct WorkspaceInfoRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct WorkspaceInfoResponse {
    pub user_id: String,
    #[serde(default)]
    #[ts(optional)]
    pub username: Option<String>,
    pub base_workspace_dir: String,
    pub user_workspace_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct WorkspaceDocSummary {
    pub kind: WorkspaceDocKind,
    pub path: String,
    pub exists: bool,
    #[ts(type = "number | null")]
    pub updated_at: Option<i64>,
    pub version_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct WorkspaceDocInfo {
    pub kind: WorkspaceDocKind,
    pub path: String,
    pub exists: bool,
    pub content: String,
    #[ts(type = "number | null")]
    pub updated_at: Option<i64>,
    pub version_token: Option<String>,
}
