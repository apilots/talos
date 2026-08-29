use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
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

#[derive(Debug, Clone, Copy, Serialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MemoryImportance {
    Low,
    Normal,
    High,
}

impl<'de> Deserialize<'de> for MemoryImportance {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "low" => Ok(Self::Low),
            "medium" | "normal" => Ok(Self::Normal),
            "high" => Ok(Self::High),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["low", "normal", "medium", "high"],
            )),
        }
    }
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

/// Selects whether a file is personal or restricted to one project context.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(tag = "type", rename_all = "camelCase")]
pub enum FileScope {
    Personal,
    Project {
        #[serde(rename = "projectId")]
        #[ts(rename = "projectId")]
        project_id: String,
    },
}

/// Public metadata for one authenticated user's ready file.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FileInfo {
    pub file_id: String,
    pub scope: FileScope,
    pub original_filename: String,
    pub mime_type: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
    pub tags: Vec<String>,
    pub metadata: BTreeMap<String, String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// Describes metadata supplied alongside one streamed file upload.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FileUploadRequest {
    pub filename: String,
    pub scope: FileScope,
    pub tags: Vec<String>,
    pub metadata: BTreeMap<String, String>,
}

/// Returns the ready file created by an upload.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FileUploadResponse {
    pub file: FileInfo,
}

/// Filters one authenticated user's files with opaque cursor pagination.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FileListRequest {
    #[serde(default)]
    #[ts(optional)]
    pub cursor: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub limit: Option<u32>,
    #[serde(default)]
    #[ts(optional)]
    pub scope: Option<FileScope>,
    #[serde(default)]
    #[ts(optional)]
    pub tags: Option<Vec<String>>,
}

/// Returns one page of ready files and the next opaque cursor.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FileListResponse {
    pub data: Vec<FileInfo>,
    pub next_cursor: Option<String>,
}

/// Selects one file owned by the authenticated user.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FileReadRequest {
    pub file_id: String,
}

/// Returns public metadata for one ready file.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FileReadResponse {
    pub file: FileInfo,
}

/// Replaces selected mutable metadata on one ready file.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FileUpdateRequest {
    pub file_id: String,
    #[serde(default)]
    #[ts(optional)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    #[ts(optional)]
    pub metadata: Option<BTreeMap<String, String>>,
}

/// Returns public metadata after a successful file update.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FileUpdateResponse {
    pub file: FileInfo,
}

/// Selects one file to delete for the authenticated user.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FileDeleteRequest {
    pub file_id: String,
}

/// Confirms the identifier of a deleted file.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct FileDeleteResponse {
    pub file_id: String,
}

#[cfg(test)]
#[path = "file_api_tests.rs"]
mod tests;
