use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use talos_types::FileScope;

/// Internal lifecycle state for one stored file.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Uploading,
    Ready,
    Deleting,
}

impl FileStatus {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Uploading => "uploading",
            Self::Ready => "ready",
            Self::Deleting => "deleting",
        }
    }
}

/// Store-owned database object for one file and its tags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecord {
    pub file_id: String,
    pub owner_user_id: String,
    pub project_id: Option<String>,
    pub location: FileLocation,
    pub original_filename: String,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub status: FileStatus,
    pub tags: Vec<String>,
    pub metadata: BTreeMap<String, String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Body coordinates confined to the FileStore/FileService boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileLocation {
    Local {
        storage_id: String,
        object_key: String,
    },
    Peer {
        peer_id: String,
    },
}
impl FileRecord {
    pub fn local_location(&self) -> talos_error::TalosResult<(&str, &str)> {
        match &self.location {
            FileLocation::Local {
                storage_id,
                object_key,
            } => Ok((storage_id, object_key)),
            FileLocation::Peer { .. } => Err(talos_error::TalosError::new(
                talos_error::ErrorCode::ServiceUnavailable,
                "File body must be read from its source FileService.",
            )),
        }
    }
}

/// Identifies a file only within its authenticated owner boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOwnerKey {
    pub file_id: String,
    pub owner_user_id: String,
}

/// Stable descending list cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileListCursor {
    pub created_at: i64,
    pub file_id: String,
}

/// Stable ascending recovery cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIncompleteCursor {
    pub updated_at: i64,
    pub file_id: String,
}

/// Store query for ready files visible to one owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileListQuery {
    pub owner_user_id: String,
    pub scope: Option<FileScope>,
    pub tags: Vec<String>,
    pub cursor: Option<FileListCursor>,
    pub limit: usize,
}

/// Atomic transition data from uploading to ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileReadyUpdate {
    pub file_id: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub updated_at: i64,
}

/// Owner-scoped mutable metadata where `None` means unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecordUpdate {
    pub tags: Option<Vec<String>>,
    pub metadata: Option<BTreeMap<String, String>>,
    pub updated_at: i64,
}
