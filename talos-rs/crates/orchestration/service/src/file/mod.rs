mod cursor;
mod peer;
mod reader;
pub use peer::FilePeerAccess;
mod service;
mod upload;
mod validation;

use std::collections::{BTreeMap, BTreeSet};
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use futures_util::Stream;
use opendal::Operator;
use talos_config::FileStorageLimits;
use talos_store::file::FileStore;
use talos_store::project::ProjectStore;

/// Transport-neutral stream consumed by one file upload.
pub type FileByteStream = Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send + 'static>>;

/// Authorized file bytes, without storage coordinates or transport encoding.
pub struct FileContent {
    pub file: talos_types::FileInfo,
    pub bytes: Bytes,
}

/// Owns file authorization, object I/O, validation, and lifecycle recovery.
///
/// Gateways pass shared request objects and byte streams to this service. The
/// service does not expose database records, OpenDAL operators, or object keys.
pub struct FileService {
    pub(super) peer_access: std::sync::RwLock<Option<Arc<dyn FilePeerAccess>>>,
    pub(super) store: Arc<dyn FileStore>,
    pub(super) projects: Arc<dyn ProjectStore>,
    pub(super) operators: BTreeMap<String, Operator>,
    pub(super) default_storage_id: String,
    pub(super) limits: FileStorageLimits,
    pub(super) allowed_mime_types: BTreeSet<String>,
}

#[cfg(test)]
#[path = "file_tests.rs"]
mod tests;
