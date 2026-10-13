//! Source FileService access for imported metadata; no object paths or bodies in sync.
use super::{FileContent, FileService};
use std::sync::Arc;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    FileDeleteRequest, FileDeleteResponse, FileReadRequest, FileUpdateRequest, FileUpdateResponse,
};

/// Caller has already authorized the original owner/project scope. Implementations
/// resolve that owner's current credentials and source peer through agent-client;
/// they must preserve errors and never resolve a peer as a local object key.
pub trait FilePeerAccess: Send + Sync {
    fn read<'a>(
        &'a self,
        owner: &'a str,
        peer: &'a str,
        request: FileReadRequest,
        max_bytes: u64,
    ) -> agent_protocol::AgentFuture<'a, TalosResult<FileContent>>;
    fn update<'a>(
        &'a self,
        owner: &'a str,
        peer: &'a str,
        request: FileUpdateRequest,
    ) -> agent_protocol::AgentFuture<'a, TalosResult<FileUpdateResponse>>;
    fn delete<'a>(
        &'a self,
        owner: &'a str,
        peer: &'a str,
        request: FileDeleteRequest,
    ) -> agent_protocol::AgentFuture<'a, TalosResult<FileDeleteResponse>>;
}
impl FileService {
    /// Composition only; transport and token refresh remain in the host's SDK adapter.
    pub fn set_peer_access(&self, access: Arc<dyn FilePeerAccess>) {
        *self.peer_access.write().expect("File peer access lock") = Some(access);
    }
    pub(super) fn peer_access(&self) -> TalosResult<Arc<dyn FilePeerAccess>> {
        self.peer_access
            .read()
            .expect("File peer access lock")
            .clone()
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "The source FileService is unavailable",
                )
            })
    }
}
