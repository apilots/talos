//! Typed source FileService operations, reachable without a Session or Turn.
use crate::AgentNodeClient;
use agent_pb as pb;
use agent_protocol::{error_pb, resources_pb};
use bytes::Bytes;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    FileDeleteRequest, FileDeleteResponse, FileInfo, FileReadRequest, FileUpdateRequest,
    FileUpdateResponse,
};

impl AgentNodeClient {
    /// Read an owned source FileService through its live management connection.
    /// The source enforces its original per-file limit; the caller supplies its
    /// existing limit, too. No object key, path or synced copy is consulted.
    pub async fn read_peer_file(
        &self,
        token: &str,
        source_peer_id: &str,
        params: FileReadRequest,
        max_bytes: u64,
    ) -> TalosResult<(FileInfo, Bytes)> {
        let file_id = params.file_id.clone();
        let mut stream =
            pb::agent_peer_file_rpc_client::AgentPeerFileRpcClient::new(self.channel.clone())
                .read_file(request(
                    token,
                    pb::PeerFileReadRequest {
                        auth: None,
                        source_peer_id: source_peer_id.into(),
                        params: Some(pb::PeerFileReadParams {
                            file_id: params.file_id,
                        }),
                    },
                )?)
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        let mut file: Option<FileInfo> = None;
        let mut bytes = Vec::new();
        while let Some(reply) = stream.message().await.map_err(crate::decode_rpc_status)? {
            match reply.result.ok_or_else(invalid)? {
                pb::peer_file_read_response::Result::File(value) if file.is_none() => {
                    let metadata = resources_pb::decode_tool_file(value)?;
                    if metadata.file_id != file_id || metadata.size_bytes > max_bytes {
                        return Err(TalosError::new(
                            ErrorCode::AttachmentLimitExceeded,
                            "Source file exceeds the caller's file limit",
                        ));
                    }
                    file = Some(metadata);
                }
                pb::peer_file_read_response::Result::Chunk(chunk) => {
                    let metadata = file.as_ref().ok_or_else(invalid)?;
                    let length = bytes.len().checked_add(chunk.len()).ok_or_else(invalid)? as u64;
                    if chunk.len() > 64 * 1024 || length > metadata.size_bytes || length > max_bytes
                    {
                        return Err(invalid());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                pb::peer_file_read_response::Result::Completed(_) => {
                    let file = file.ok_or_else(invalid)?;
                    if file.size_bytes != bytes.len() as u64 {
                        return Err(invalid());
                    }
                    return Ok((file, Bytes::from(bytes)));
                }
                pb::peer_file_read_response::Result::Failure(value) => {
                    return Err(error_pb::decode_failure(value)?)
                }
                _ => return Err(invalid()),
            }
        }
        Err(invalid())
    }
    /// Update the source FileService metadata under the current account. None
    /// preserves a field; an explicit empty value clears it. The SDK neither
    /// materializes metadata nor mutates a local copy of the source object.
    pub async fn update_peer_file(
        &self,
        token: &str,
        source_peer_id: &str,
        params: FileUpdateRequest,
    ) -> TalosResult<FileUpdateResponse> {
        let response =
            pb::agent_peer_file_rpc_client::AgentPeerFileRpcClient::new(self.channel.clone())
                .update_file(request(
                    token,
                    pb::PeerFileUpdateRequest {
                        auth: None,
                        source_peer_id: source_peer_id.into(),
                        params: Some(pb::PeerFileUpdateParams {
                            file_id: params.file_id,
                            tags: params.tags.map(|values| pb::OptionalTags { values }),
                            metadata: params.metadata.map(|values| pb::OptionalFileMetadata {
                                values: values.into_iter().collect(),
                            }),
                        }),
                    },
                )?)
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        match response.result.ok_or_else(invalid)? {
            pb::peer_file_update_response::Result::File(value) => Ok(FileUpdateResponse {
                file: resources_pb::decode_tool_file(value)?,
            }),
            pb::peer_file_update_response::Result::Failure(value) => {
                Err(error_pb::decode_failure(value)?)
            }
        }
    }
    /// Delete through the source FileService authorization and object lifecycle.
    /// Unavailable sources fail explicitly; callers must persist/sync resulting
    /// facts through their original business transaction, outside this SDK.
    pub async fn delete_peer_file(
        &self,
        token: &str,
        source_peer_id: &str,
        params: FileDeleteRequest,
    ) -> TalosResult<FileDeleteResponse> {
        let response =
            pb::agent_peer_file_rpc_client::AgentPeerFileRpcClient::new(self.channel.clone())
                .delete_file(request(
                    token,
                    pb::PeerFileDeleteRequest {
                        auth: None,
                        source_peer_id: source_peer_id.into(),
                        params: Some(pb::PeerFileDeleteParams {
                            file_id: params.file_id,
                        }),
                    },
                )?)
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        match response.result.ok_or_else(invalid)? {
            pb::peer_file_delete_response::Result::FileId(file_id) => {
                Ok(FileDeleteResponse { file_id })
            }
            pb::peer_file_delete_response::Result::Failure(value) => {
                Err(error_pb::decode_failure(value)?)
            }
        }
    }
}
fn request<T>(token: &str, body: T) -> TalosResult<tonic::Request<T>> {
    if token.is_empty() {
        return Err(invalid());
    }
    let mut request = tonic::Request::new(body);
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {token}").parse().map_err(|_| invalid())?,
    );
    request.set_timeout(std::time::Duration::from_secs(30));
    Ok(request)
}
fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Peer file RPC payload is invalid",
    )
}
