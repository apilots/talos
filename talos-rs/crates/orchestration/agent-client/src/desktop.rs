//! Dedicated media channels to the already configured trusted Agent ingress.
//! Bounded byte queues are independent of Connect and task/resource RPC windows.
use agent_pb as pb;
use agent_protocol::{desktop_pb, error_pb, node_pb, NodeAuth};
use futures_util::{Stream, StreamExt};
use talos_browser::DesktopTransport;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{DesktopStreamBinding, WorkerDesktopOpen};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tonic::{transport::Channel, Request, Status};

pub struct DesktopReservation {
    pub nonce: String,
    pub ticket: String,
    pub expires_at: i64,
}

impl crate::AgentNodeClient {
    pub async fn reserve_desktop(
        &self,
        token: &str,
        node_id: String,
        binding: DesktopStreamBinding,
    ) -> TalosResult<DesktopReservation> {
        let mut request = Request::new(pb::DesktopReserveRequest {
            node_id,
            binding: Some(desktop_pb::encode_binding(binding)?),
        });
        authorize(&mut request, token)?;
        request.set_timeout(std::time::Duration::from_secs(10));
        let response = client(self.channel.clone())
            .reserve(request)
            .await
            .map_err(crate::decode_rpc_status)?
            .into_inner();
        match response.result.ok_or_else(invalid)? {
            pb::desktop_reserve_response::Result::Value(value) => {
                if uuid::Uuid::parse_str(&value.nonce).is_err()
                    || uuid::Uuid::parse_str(&value.ticket).is_err()
                {
                    return Err(invalid());
                }
                Ok(DesktopReservation {
                    nonce: value.nonce,
                    ticket: value.ticket,
                    expires_at: value.expires_at,
                })
            }
            pb::desktop_reserve_response::Result::Failure(error) => {
                Err(error_pb::decode_failure(error)?)
            }
        }
    }
    pub async fn view_desktop(
        &self,
        token: &str,
        node_id: String,
        stream_id: String,
        ticket: String,
    ) -> TalosResult<DesktopTransport> {
        let (outgoing, input) = mpsc::channel::<Vec<u8>>(64);
        let frames = tokio_stream::once(pb::DesktopViewerFrame {
            body: Some(pb::desktop_viewer_frame::Body::Hello(
                pb::DesktopViewerHello {
                    node_id,
                    stream_id,
                    ticket,
                },
            )),
        })
        .chain(
            ReceiverStream::new(input).map(|data| pb::DesktopViewerFrame {
                body: Some(pb::desktop_viewer_frame::Body::Chunk(pb::DesktopChunk {
                    data,
                })),
            }),
        );
        let mut request = Request::new(frames);
        authorize(&mut request, token)?;
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            client(self.channel.clone()).viewer(request),
        )
        .await
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::Timeout,
                "Desktop viewer admission timed out.",
                error,
            )
        })?
        .map_err(crate::decode_rpc_status)?
        .into_inner();
        Ok(receive(outgoing, response))
    }
}
pub(crate) async fn worker(
    channel: Channel,
    auth: NodeAuth,
    request: WorkerDesktopOpen,
) -> TalosResult<DesktopTransport> {
    let (outgoing, input) = mpsc::channel::<Vec<u8>>(64);
    let frames = tokio_stream::once(pb::DesktopWorkerFrame {
        body: Some(pb::desktop_worker_frame::Body::Hello(
            pb::DesktopWorkerHello {
                auth: Some(node_pb::encode_auth(auth)),
                binding: Some(desktop_pb::encode_binding(request.binding)?),
                nonce: request.nonce,
            },
        )),
    })
    .chain(
        ReceiverStream::new(input).map(|data| pb::DesktopWorkerFrame {
            body: Some(pb::desktop_worker_frame::Body::Chunk(pb::DesktopChunk {
                data,
            })),
        }),
    );
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client(channel).worker(Request::new(frames)),
    )
    .await
    .map_err(|error| {
        TalosError::from_source(
            ErrorCode::Timeout,
            "Desktop Worker admission timed out.",
            error,
        )
    })?
    .map_err(crate::decode_rpc_status)?
    .into_inner();
    Ok(receive(outgoing, response))
}
fn receive<S>(outgoing: mpsc::Sender<Vec<u8>>, response: S) -> DesktopTransport
where
    S: Stream<Item = Result<pb::DesktopChunk, Status>> + Send + 'static,
{
    let mut response = Box::pin(response);
    let (sender, incoming) = mpsc::channel(64);
    let stopped = CancellationToken::new();
    let end = stopped.clone();
    tokio::spawn(async move {
        let result = async {
            loop {
                let chunk =
                    tokio::select! {_=end.cancelled()=>return Ok(()),chunk=response.next()=>chunk};
                let Some(chunk) = chunk else {
                    return Ok(());
                };
                let chunk = chunk.map_err(crate::decode_rpc_status)?.data;
                if chunk.is_empty() || chunk.len() > 64 * 1024 {
                    return Err(invalid());
                }
                tokio::time::timeout(std::time::Duration::from_secs(2), sender.send(Ok(chunk)))
                    .await
                    .map_err(|error| {
                        TalosError::from_source(
                            ErrorCode::Timeout,
                            "Desktop consumer is too slow.",
                            error,
                        )
                    })?
                    .map_err(|_| unavailable())?;
            }
        }
        .await;
        if let Err(error) = result {
            let _ = sender.try_send(Err(error));
        }
        end.cancel();
    });
    DesktopTransport {
        incoming,
        outgoing,
        stopped,
    }
}
fn authorize<T>(request: &mut Request<T>, token: &str) -> TalosResult<()> {
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {token}").parse().map_err(|_| invalid())?,
    );
    Ok(())
}
fn client(channel: Channel) -> pb::agent_desktop_media_client::AgentDesktopMediaClient<Channel> {
    pb::agent_desktop_media_client::AgentDesktopMediaClient::new(channel)
        .max_decoding_message_size(65 * 1024)
        .max_encoding_message_size(65 * 1024)
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
