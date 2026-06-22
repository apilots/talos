use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct FsDirectoryReadRequest {
    #[serde(default)]
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct FsDirectoryCreateRequest {
    pub parent: PathBuf,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct FsDirectorySnapshot {
    pub path: PathBuf,
    #[serde(default)]
    pub parent: Option<PathBuf>,
    pub entries: Vec<FsDirectoryEntry>,
    pub readable: bool,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct FsDirectoryReadResponse {
    pub directory: FsDirectorySnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct FsDirectoryEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: FsEntryKind,
    pub readable: bool,
    pub writable: bool,
    pub hidden: bool,
    pub is_symlink: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FsEntryKind {
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct FsDirectoryCreateResponse {
    pub path: PathBuf,
}
