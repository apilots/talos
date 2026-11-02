//! Source FileService retains original authorization, object I/O and lifecycle.
use super::*;
use agent_protocol::resources_pb;
use talos_types::{FileDeleteRequest, FileReadRequest, FileUpdateRequest};

impl CoordinatorResources {
    pub(super) async fn dispatch_peer_file(
        &self,
        request: pb::worker_rpc_call::Request,
    ) -> Result<AgentResourceResponses, Status> {
        let auth = agent_protocol::connection_pb::auth(&request).ok_or_else(invalid)?;
        // This is the source connection's own account credential, supplied by
        // the Gateway and checked by the SDK. Revalidate its current identity.
        let identity = self
            .auth
            .verify_access_token(&auth.token)
            .await
            .map_err(status)?;
        if identity.auth_disabled {
            return Err(status(TalosError::new(
                ErrorCode::Unauthenticated,
                "Peer file access requires a signed-in account",
            )));
        }
        let actor = Actor::regular(identity.user_id);
        use pb::worker_rpc_call::Request as Call;
        use pb::worker_rpc_result::Response as Reply;
        match request {
            Call::PeerFileRead(request) => {
                let params = request.params.ok_or_else(invalid)?;
                let result = match params.range {
                    Some(range) => {
                        self.files
                            .read_preview_range(
                                &actor,
                                &params.file_id,
                                range.offset..range.offset.saturating_add(u64::from(range.limit)),
                            )
                            .await
                    }
                    None => {
                        self.files
                            .read_content(
                                &actor,
                                FileReadRequest {
                                    file_id: params.file_id,
                                },
                            )
                            .await
                    }
                };
                let content = match result {
                    Ok(content) => content,
                    Err(error) => {
                        let response = pb::PeerFileReadResponse {
                            result: Some(pb::peer_file_read_response::Result::Failure(failure(
                                error,
                            ))),
                        };
                        return Ok(Box::pin(tokio_stream::once(Ok(Reply::PeerFileRead(
                            Box::new(response),
                        )))));
                    }
                };
                let metadata = resources_pb::encode_tool_file(content.file).map_err(status)?;
                let first = tokio_stream::once(Ok(Reply::PeerFileRead(Box::new(
                    pb::PeerFileReadResponse {
                        result: Some(pb::peer_file_read_response::Result::File(metadata)),
                    },
                ))));
                let bytes = futures_util::stream::unfold(
                    (content.bytes, 0_usize),
                    |(bytes, offset)| async move {
                        if offset == bytes.len() {
                            return None;
                        }
                        let end = offset.saturating_add(64 * 1024).min(bytes.len());
                        let reply = Reply::PeerFileRead(Box::new(pb::PeerFileReadResponse {
                            result: Some(pb::peer_file_read_response::Result::Chunk(
                                bytes.slice(offset..end).to_vec(),
                            )),
                        }));
                        Some((Ok(reply), (bytes, end)))
                    },
                );
                let completed = tokio_stream::once(Ok(Reply::PeerFileRead(Box::new(
                    pb::PeerFileReadResponse {
                        result: Some(pb::peer_file_read_response::Result::Completed(pb::Empty {})),
                    },
                ))));
                Ok(Box::pin(first.chain(bytes).chain(completed)))
            }
            Call::PeerFileUpdate(request) => {
                let params = request.params.ok_or_else(invalid)?;
                let result = self
                    .files
                    .update(
                        &actor,
                        FileUpdateRequest {
                            file_id: params.file_id,
                            tags: params.tags.map(|tags| tags.values),
                            metadata: params
                                .metadata
                                .map(|metadata| metadata.values.into_iter().collect()),
                        },
                    )
                    .await;
                let result = match result
                    .and_then(|result| resources_pb::encode_tool_file(result.file))
                {
                    Ok(file) => pb::peer_file_update_response::Result::File(file),
                    Err(error) => pb::peer_file_update_response::Result::Failure(failure(error)),
                };
                Ok(Box::pin(tokio_stream::once(Ok(Reply::PeerFileUpdate(
                    Box::new(pb::PeerFileUpdateResponse {
                        result: Some(result),
                    }),
                )))))
            }
            Call::PeerFileDelete(request) => {
                let params = request.params.ok_or_else(invalid)?;
                let result = match self
                    .files
                    .delete(
                        &actor,
                        FileDeleteRequest {
                            file_id: params.file_id,
                        },
                    )
                    .await
                {
                    Ok(result) => pb::peer_file_delete_response::Result::FileId(result.file_id),
                    Err(error) => pb::peer_file_delete_response::Result::Failure(failure(error)),
                };
                Ok(Box::pin(tokio_stream::once(Ok(Reply::PeerFileDelete(
                    Box::new(pb::PeerFileDeleteResponse {
                        result: Some(result),
                    }),
                )))))
            }
            _ => Err(invalid()),
        }
    }
}
fn status(error: TalosError) -> Status {
    talos_logging::report_error(&error);
    error_pb::encode_status(&error)
}
