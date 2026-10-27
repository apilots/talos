//! Authentication and framing adapters for the shared bounded desktop relay.
use super::*;
use agent_protocol::desktop_pb;
use futures_util::{Stream, StreamExt};
use std::{pin::Pin, time::Duration};
use talos_browser::DesktopTransport;
use talos_types::WorkerDesktopOpen;
use tokio::{sync::mpsc, time::timeout};
use tokio_stream::wrappers::ReceiverStream;

#[path = "desktop_relay.rs"]
pub(crate) mod relay;
use relay::{guard_transport, DesktopRelay};

type MediaStream = Pin<Box<dyn Stream<Item = Result<pb::DesktopChunk, Status>> + Send>>;

#[derive(Clone)]
pub(super) struct DesktopMediaRpc {
    management: NodeManagementRpc,
    relay: DesktopRelay,
}
impl DesktopMediaRpc {
    pub(super) fn new(management: NodeManagementRpc, shutdown: CancellationToken) -> Self {
        Self {
            management,
            relay: DesktopRelay::new(shutdown),
        }
    }
    async fn owner(&self, actor: &Actor, node: &str) -> TalosResult<()> {
        let found = self
            .management
            .nodes
            .get_node(
                actor,
                agent_protocol::GetNodeRequest {
                    node_id: node.into(),
                },
            )
            .await?;
        if found.owner_user_id.as_deref() != Some(&actor.user_id) {
            return Err(denied());
        }
        Ok(())
    }

    /// Only adapts wire framing. Reservation ownership, expiry, flow bounds and
    /// candidate replacement remain in the shared relay.
    fn stream<S>(&self, mut input: Pin<Box<S>>, mut transport: DesktopTransport) -> MediaStream
    where
        S: Stream<Item = Result<Vec<u8>, Status>> + Send + ?Sized + 'static,
    {
        let (sender, output) = mpsc::channel(64);
        tokio::spawn(async move {
            let result = async {
                loop {
                    tokio::select! {
                        _=transport.stopped.cancelled()=>break,
                        _=sender.closed()=>break,
                        chunk=input.next()=>{
                            let Some(chunk)=chunk else {break;};
                            let data=chunk.map_err(error_pb::decode_status)?;
                            timeout(Duration::from_secs(2),transport.outgoing.send(data)).await
                                .map_err(|error|TalosError::from_source(ErrorCode::Timeout,"Desktop consumer exceeded its bounded queue.",error))?
                                .map_err(|_|unavailable())?;
                        },
                        chunk=transport.incoming.recv()=>{
                            let Some(chunk)=chunk else {break;};
                            let data=match chunk {
                                Ok(data)=>data,
                                Err(error)=>{
                                    // The relay owns and already reported this failure.
                                    let _=sender.try_send(Err(error_pb::encode_status(&error)));
                                    break;
                                }
                            };
                            timeout(Duration::from_secs(2),sender.send(Ok(pb::DesktopChunk{data}))).await
                                .map_err(|error|TalosError::from_source(ErrorCode::Timeout,"Desktop consumer exceeded its bounded queue.",error))?
                                .map_err(|_|unavailable())?;
                        },
                    }
                }
                Ok::<_,TalosError>(())
            }.await;
            if let Err(error) = result {
                talos_logging::report_error(&error);
                let _ = sender.try_send(Err(error_pb::encode_status(&error)));
            }
            drop(transport);
        });
        Box::pin(ReceiverStream::new(output))
    }
}

#[tonic::async_trait]
impl pb::agent_desktop_media_server::AgentDesktopMedia for DesktopMediaRpc {
    type WorkerStream = MediaStream;
    type ViewerStream = MediaStream;
    async fn reserve(
        &self,
        request: Request<pb::DesktopReserveRequest>,
    ) -> Result<Response<pb::DesktopReserveResponse>, Status> {
        let result = async {
            let actor = self.management.actor(&request).await?;
            let request = request.into_inner();
            self.owner(&actor, &request.node_id).await?;
            let binding = desktop_pb::decode_binding(request.binding)?;
            let reservation = self
                .relay
                .reserve(actor.user_id, request.node_id, binding)
                .await?;
            Ok(pb::DesktopReservation {
                nonce: reservation.nonce,
                ticket: reservation.ticket,
                expires_at: reservation.expires_at,
            })
        }
        .await;
        Ok(Response::new(pb::DesktopReserveResponse {
            result: Some(match result {
                Ok(value) => pb::desktop_reserve_response::Result::Value(value),
                Err(error) => {
                    talos_logging::report_error(&error);
                    pb::desktop_reserve_response::Result::Failure(error_pb::encode_failure(&error))
                }
            }),
        }))
    }
    async fn cancel(
        &self,
        request: Request<pb::DesktopCancelRequest>,
    ) -> Result<Response<pb::DesktopCancelResponse>, Status> {
        let result = async {
            let actor = self.management.actor(&request).await?;
            let request = request.into_inner();
            self.owner(&actor, &request.node_id).await?;
            self.relay
                .cancel(
                    &actor.user_id,
                    &request.node_id,
                    &request.stream_id,
                    &request.ticket,
                )
                .await
        }
        .await;
        Ok(Response::new(pb::DesktopCancelResponse {
            result: Some(match result {
                Ok(()) => pb::desktop_cancel_response::Result::Value(pb::Empty {}),
                Err(error) => {
                    talos_logging::report_error(&error);
                    pb::desktop_cancel_response::Result::Failure(error_pb::encode_failure(&error))
                }
            }),
        }))
    }
    async fn worker(
        &self,
        request: Request<tonic::Streaming<pb::DesktopWorkerFrame>>,
    ) -> Result<Response<Self::WorkerStream>, Status> {
        let mut input = request.into_inner();
        let first = first(&mut input).await?;
        let Some(pb::desktop_worker_frame::Body::Hello(hello)) = first.body else {
            return Err(status(invalid()));
        };
        let auth = node_pb::decode_auth(hello.auth).map_err(status)?;
        let owner = self
            .management
            .nodes
            .authorize_lease_owner(&auth)
            .await
            .map_err(status)?;
        let binding = desktop_pb::decode_binding(hello.binding).map_err(status)?;
        let transport = self
            .relay
            .worker(
                &owner,
                &auth.node_id,
                WorkerDesktopOpen {
                    binding,
                    nonce: hello.nonce,
                },
            )
            .await
            .map_err(status)?;
        let nodes = self.management.nodes.clone();
        let transport = guard_transport(
            transport,
            Arc::new(move || {
                let nodes = nodes.clone();
                let auth = auth.clone();
                let owner = owner.clone();
                Box::pin(async move {
                    if nodes.authorize_lease_owner(&auth).await? != owner {
                        return Err(denied());
                    }
                    Ok(())
                })
            }),
        );
        let input = input.map(|frame| match frame?.body {
            Some(pb::desktop_worker_frame::Body::Chunk(chunk)) => Ok(chunk.data),
            _ => Err(error_pb::encode_status(&invalid())),
        });
        Ok(Response::new(self.stream(Box::pin(input), transport)))
    }
    async fn viewer(
        &self,
        request: Request<tonic::Streaming<pb::DesktopViewerFrame>>,
    ) -> Result<Response<Self::ViewerStream>, Status> {
        let actor = self.management.actor(&request).await.map_err(status)?;
        let mut input = request.into_inner();
        let first = first(&mut input).await?;
        let Some(pb::desktop_viewer_frame::Body::Hello(hello)) = first.body else {
            return Err(status(invalid()));
        };
        self.owner(&actor, &hello.node_id).await.map_err(status)?;
        let transport = self
            .relay
            .viewer(
                &actor.user_id,
                &hello.node_id,
                &hello.stream_id,
                &hello.ticket,
            )
            .await
            .map_err(status)?;
        let rpc = self.clone();
        let transport = guard_transport(
            transport,
            Arc::new(move || {
                let rpc = rpc.clone();
                let actor = actor.clone();
                let node = hello.node_id.clone();
                Box::pin(async move {
                    let identity = rpc
                        .management
                        .auth_service
                        .verify_access_token(actor.access_token.as_deref().ok_or_else(denied)?)
                        .await?;
                    if identity.user_id != actor.user_id {
                        return Err(denied());
                    }
                    rpc.owner(&actor, &node).await
                })
            }),
        );
        let input = input.map(|frame| match frame?.body {
            Some(pb::desktop_viewer_frame::Body::Chunk(chunk)) => Ok(chunk.data),
            _ => Err(error_pb::encode_status(&invalid())),
        });
        Ok(Response::new(self.stream(Box::pin(input), transport)))
    }
}

async fn first<T: Send + 'static>(input: &mut tonic::Streaming<T>) -> Result<T, Status> {
    timeout(Duration::from_secs(5), input.message())
        .await
        .map_err(|_| status(invalid()))??
        .ok_or_else(|| status(invalid()))
}
fn status(error: TalosError) -> Status {
    talos_logging::report_error(&error);
    error_pb::encode_status(&error)
}
fn denied() -> TalosError {
    TalosError::new(
        ErrorCode::PermissionDenied,
        "Desktop media admission was rejected.",
    )
}
fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Desktop media framing is invalid.",
    )
}
fn unavailable() -> TalosError {
    TalosError::new(
        ErrorCode::ServiceUnavailable,
        "Desktop media is unavailable.",
    )
}
