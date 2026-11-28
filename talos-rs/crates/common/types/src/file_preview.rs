//! One file-preview contract for registered attachments and session-bound files.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ts_rs::TS;

/// Maximum payload of one preview read, shared by source owners and adapters.
pub const FILE_PREVIEW_MAX_READ_BYTES: u32 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(tag = "kind", rename_all = "camelCase")]
pub enum FilePreviewSource {
    File {
        #[ts(rename = "fileId")]
        file_id: String,
        /// Conversion Worker. Omission selects only the embedded Computer.
        #[ts(rename = "nodeId", optional)]
        node_id: Option<String>,
    },
    SessionPath {
        #[ts(rename = "sessionId")]
        session_id: String,
        path: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilePreviewPrepareRequest {
    pub source: FilePreviewSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilePreviewMetadata {
    pub name: String,
    pub mime_type: String,
    #[ts(type = "number")]
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilePreviewDescriptor {
    /// Ephemeral scoped capability. Never persist or share this identifier.
    pub preview_id: String,
    /// URL path relative to the preview root; never a host filesystem path.
    pub entry_path: String,
    /// Configured per-preview HTTP origin; native transports supply their own URL adapter.
    pub resource_base_url: Option<String>,
    pub metadata: FilePreviewMetadata,
    /// Additional document capability; absent means browser/text presentation of the source.
    pub document: Option<FilePreviewDocument>,
    #[ts(type = "number")]
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(tag = "kind", rename_all = "camelCase")]
pub enum FilePreviewDocument {
    Pdf {
        #[serde(rename = "resourcePath")]
        #[ts(rename = "resourcePath")]
        resource_path: String,
    },
    Unavailable {
        reason: FilePreviewDocumentUnavailable,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum FilePreviewDocumentUnavailable {
    ConverterUnavailable,
    ConversionFailed,
    TooLarge,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilePreviewReadRequest {
    pub preview_id: String,
    pub resource_path: String,
    #[ts(type = "number")]
    pub offset: u64,
    /// At most one MiB per request. Zero reads metadata only.
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilePreviewContent {
    pub metadata: FilePreviewMetadata,
    #[ts(type = "number")]
    pub offset: u64,
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilePreviewCloseRequest {
    pub preview_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilePreviewCloseResponse {
    pub closed: bool,
}

/// Internal resource data shared by service, filesystem and Worker owners.
/// JSON transports encode only at their boundary, like FileContentResponse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePreviewBytes {
    pub metadata: FilePreviewMetadata,
    pub offset: u64,
    pub data: bytes::Bytes,
}

/// Preview operations on the selected Worker. The service derives Session roots
/// and authorizes staged product File inputs. Handles own source and converted
/// data until close or expiry; Session reads never reopen the root path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerPreviewRequest {
    Open {
        root: PathBuf,
        path: PathBuf,
        expires_at: i64,
    },
    /// Open a bounded conversion input for an authorized product File.
    OpenFile {
        metadata: FilePreviewMetadata,
        expires_at: i64,
        max_bytes: u64,
    },
    /// Append one contiguous input chunk; at most FILE_PREVIEW_MAX_READ_BYTES.
    Write {
        handle: String,
        offset: u64,
        data: bytes::Bytes,
    },
    /// Convert on the handle owner. Both input and output obey max_bytes.
    PrepareDocument {
        handle: String,
        max_bytes: u64,
    },
    Read {
        handle: String,
        path: PathBuf,
        offset: u64,
        limit: u32,
    },
    Close {
        handle: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerPreviewResponse {
    Opened {
        handle: String,
        resource_path: String,
        metadata: FilePreviewMetadata,
    },
    Written,
    Document(FilePreviewDocument),
    Content(FilePreviewBytes),
    Closed,
}
