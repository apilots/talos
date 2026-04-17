use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ExpectedFileVersion {
    MustNotExist,
    Match { version_token: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocUpdateRequest {
    pub content: String,
    pub base_version: ExpectedFileVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryFileSummary {
    pub date: String,
    pub path: String,
    pub section_count: usize,
    pub updated_at: Option<i64>,
    pub version_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryFileDetail {
    pub date: String,
    pub path: String,
    pub section_count: usize,
    pub content: String,
    pub updated_at: Option<i64>,
    pub version_token: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceDocSummary {
    pub kind: WorkspaceDocKind,
    pub path: String,
    pub exists: bool,
    pub updated_at: Option<i64>,
    pub version_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceDocDetail {
    pub kind: WorkspaceDocKind,
    pub path: String,
    pub exists: bool,
    pub content: String,
    pub updated_at: Option<i64>,
    pub version_token: Option<String>,
}
