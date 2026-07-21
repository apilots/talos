use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Maximum canonical size of one workspace MEMORY.md file.
pub const MEMORY_FILE_MAX_CHARS: usize = 8_000;

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
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemoryFileReadResponse {
    pub file: MemoryFileDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemoryFileWriteRequest {
    pub date: String,
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
#[serde(rename_all = "camelCase")]
pub struct MemorySearchRequest {
    pub query: String,
    #[serde(default)]
    #[ts(optional)]
    pub limit: Option<u32>,
    #[serde(default)]
    #[ts(optional)]
    pub kinds: Option<Vec<MemoryKind>>,
    #[serde(default)]
    #[ts(optional)]
    #[ts(type = "number | null")]
    pub since_at: Option<i64>,
    #[serde(default)]
    #[ts(optional)]
    #[ts(type = "number | null")]
    pub until_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchResponse {
    pub data: Vec<MemorySearchHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchHit {
    pub memory_id: String,
    pub title: String,
    pub preview: String,
    pub kind: MemoryKind,
    pub importance: MemoryImportance,
    #[ts(type = "number")]
    pub occurred_at: i64,
    pub rank: u32,
    pub source: MemorySourceInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryGetRequest {
    pub memory_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryGetResponse {
    pub memory: MemoryInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    pub memory_id: String,
    pub title: String,
    pub body: String,
    pub kind: MemoryKind,
    pub importance: MemoryImportance,
    #[ts(type = "number")]
    pub occurred_at: i64,
    pub source: MemorySourceInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemorySourceInfo {
    pub date: String,
    pub session_id: Option<String>,
    #[ts(type = "number | null")]
    pub from_seq: Option<u64>,
    #[ts(type = "number | null")]
    pub until_seq: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MemoryKind {
    Preference,
    Fact,
    Decision,
    Task,
    Episode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MemoryImportance {
    Low,
    Normal,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemoryFileSummary {
    pub date: String,
    pub section_count: usize,
    #[ts(type = "number | null")]
    pub updated_at: Option<i64>,
    pub version_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct MemoryFileDetail {
    pub date: String,
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
