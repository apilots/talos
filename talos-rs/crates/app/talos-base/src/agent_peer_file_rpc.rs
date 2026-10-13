//! Account-authenticated FileService requests to a live management source peer.
use super::*;
use futures_util::StreamExt;

pub(super) struct PeerFileRpc {
    pub(super) management: NodeManagementRpc,
    pub(super) routes: ResourceRoutes,
}
#[tonic::async_trait]
impl pb::agent_peer_file_rpc_server::AgentPeerFileRpc for PeerFileRpc {
    type ReadFileStream = std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<pb::PeerFileReadResponse, Status>> + Send>,
    >;
    async fn read_file(
        &self,
        request: Request<pb::PeerFileReadRequest>,
    ) -> Result<Response<Self::ReadFileStream>, Status> {
        let actor = self.management.actor(&request).await.map_err(status)?;
        let source = request.get_ref().source_peer_id.clone();
        let replies = self
            .routes
            .forward_peer(
                &actor.user_id,
                &source,
                pb::worker_rpc_call::Request::PeerFileRead(Box::new(request.into_inner())),
            )
            .await?;
        Ok(Response::new(Box::pin(replies.map(|reply| match reply? {
            pb::worker_rpc_result::Response::PeerFileRead(value) => Ok(*value),
            _ => Err(invalid()),
        }))))
    }
    async fn update_file(
        &self,
        request: Request<pb::PeerFileUpdateRequest>,
    ) -> Result<Response<pb::PeerFileUpdateResponse>, Status> {
        let actor = self.management.actor(&request).await.map_err(status)?;
        let source = request.get_ref().source_peer_id.clone();
        let mut replies = self
            .routes
            .forward_peer(
                &actor.user_id,
                &source,
                pb::worker_rpc_call::Request::PeerFileUpdate(Box::new(request.into_inner())),
            )
            .await?;
        let reply = replies.next().await.transpose()?.ok_or_else(invalid)?;
        let pb::worker_rpc_result::Response::PeerFileUpdate(value) = reply else {
            return Err(invalid());
        };
        Ok(Response::new(*value))
    }
    async fn delete_file(
        &self,
        request: Request<pb::PeerFileDeleteRequest>,
    ) -> Result<Response<pb::PeerFileDeleteResponse>, Status> {
        let actor = self.management.actor(&request).await.map_err(status)?;
        let source = request.get_ref().source_peer_id.clone();
        let mut replies = self
            .routes
            .forward_peer(
                &actor.user_id,
                &source,
                pb::worker_rpc_call::Request::PeerFileDelete(Box::new(request.into_inner())),
            )
            .await?;
        let reply = replies.next().await.transpose()?.ok_or_else(invalid)?;
        let pb::worker_rpc_result::Response::PeerFileDelete(value) = reply else {
            return Err(invalid());
        };
        Ok(Response::new(*value))
    }
}
fn status(error: TalosError) -> Status {
    talos_logging::report_error(&error);
    error_pb::encode_status(&error)
}
fn invalid() -> Status {
    status(TalosError::new(
        ErrorCode::InvalidRequest,
        "Peer file carrier is invalid",
    ))
}
