//! Separate, bounded media relay at the existing authenticated Agent ingress.
use super::*;
use agent_protocol::desktop_pb;
use futures_util::{Stream, StreamExt};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use talos_types::{BrowserInputMode, DesktopStreamBinding};
use tokio::{
    sync::{mpsc, Mutex},
    time::timeout,
};
use tokio_stream::wrappers::ReceiverStream;

type MediaStream = Pin<Box<dyn Stream<Item = Result<pb::DesktopChunk, Status>> + Send>>;
struct Reservation {
    owner: String,
    node: String,
    binding: DesktopStreamBinding,
    nonce: [u8; 32],
    ticket: [u8; 32],
    expires: Instant,
    worker_attached: AtomicBool,
    viewer_attached: AtomicBool,
    to_worker: mpsc::Sender<Result<pb::DesktopChunk, Status>>,
    to_viewer: mpsc::Sender<Result<pb::DesktopChunk, Status>>,
    worker_output: Mutex<Option<mpsc::Receiver<Result<pb::DesktopChunk, Status>>>>,
    viewer_output: Mutex<Option<mpsc::Receiver<Result<pb::DesktopChunk, Status>>>>,
    stopped: CancellationToken,
}
#[derive(Clone)]
pub(super) struct DesktopMediaRpc {
    management: NodeManagementRpc,
    reservations: Arc<Mutex<HashMap<String, Arc<Reservation>>>>,
    shutdown: CancellationToken,
}
impl DesktopMediaRpc {
    pub(super) fn new(management: NodeManagementRpc, shutdown: CancellationToken) -> Self {
        Self {
            management,
            reservations: Arc::default(),
            shutdown,
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
    async fn entry(&self, id: &str) -> TalosResult<Arc<Reservation>> {
        self.reservations
            .lock()
            .await
            .get(id)
            .filter(|entry| !entry.stopped.is_cancelled())
            .cloned()
            .ok_or_else(denied)
    }
    async fn remove(&self, entry: &Reservation) {
        entry.stopped.cancel();
        self.reservations
            .lock()
            .await
            .remove(&entry.binding.stream_id);
    }
    fn forward<S>(
        &self,
        mut input: Pin<Box<S>>,
        entry: Arc<Reservation>,
        worker: bool,
        auth: Option<NodeAuth>,
        actor: Option<Actor>,
    ) where
        S: Stream<Item = Result<Vec<u8>, Status>> + Send + ?Sized + 'static,
    {
        let relay = self.clone();
        tokio::spawn(async move {
            let sender = if worker {
                entry.to_viewer.clone()
            } else {
                entry.to_worker.clone()
            };
            let receiving = if worker {
                entry.to_worker.clone()
            } else {
                entry.to_viewer.clone()
            };
            let mut check = tokio::time::interval(Duration::from_secs(1));
            let result=async {
                loop {
                    tokio::select! {
                        biased;
                        _=entry.stopped.cancelled()=>break,
                        _=relay.shutdown.cancelled()=>break,
                        _=receiving.closed()=>break,
                        _=check.tick()=>{
                            if let Some(auth)=&auth {
                                if relay.management.nodes.authorize_lease_owner(auth).await?!=entry.owner {return Err(denied());}
                            }
                            if let Some(actor)=&actor {
                                let identity=relay.management.auth_service.verify_access_token(actor.access_token.as_deref().ok_or_else(denied)?).await?;
                                if identity.user_id!=entry.owner {return Err(denied());}
                                relay.owner(actor,&entry.node).await?;
                            }
                            if entry.expires<=Instant::now() && !entry.viewer_attached.load(Ordering::Acquire) {return Err(denied());}
                        },
                        chunk=input.next()=>{
                            let Some(chunk)=chunk else {break;};
                            let data=chunk.map_err(error_pb::decode_status)?;
                            if data.is_empty() || data.len()>64*1024 {return Err(invalid());}
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
            relay.remove(&entry).await;
        });
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
            let mut entries = self.reservations.lock().await;
            entries.retain(|_, entry| {
                !entry.stopped.is_cancelled()
                    && (entry.expires > Instant::now()
                        || entry.viewer_attached.load(Ordering::Acquire))
            });
            // Reserve a bounded candidate without touching the current viewer.
            // Only an authenticated Worker hello proves authoritative admission.
            let node_entries: Vec<_> = entries
                .values()
                .filter(|e| e.node == request.node_id)
                .collect();
            let same_viewer = node_entries
                .iter()
                .filter(|e| e.binding.viewer_id == binding.viewer_id)
                .count();
            let viewers: std::collections::HashSet<_> =
                node_entries.iter().map(|e| &e.binding.viewer_id).collect();
            if entries.len() >= 64
                || (same_viewer == 0 && viewers.len() >= 2)
                || same_viewer >= 2
                || entries.contains_key(&binding.stream_id)
                || node_entries.iter().any(|entry| {
                    entry.binding.viewer_id != binding.viewer_id
                        && entry.binding.input_mode == BrowserInputMode::Control
                        && binding.input_mode == BrowserInputMode::Control
                })
            {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "Desktop viewer capacity is exhausted.",
                ));
            }
            let nonce = uuid::Uuid::new_v4().to_string();
            let ticket = uuid::Uuid::new_v4().to_string();
            let (to_worker, worker_output) = mpsc::channel(64);
            let (to_viewer, viewer_output) = mpsc::channel(64);
            entries.insert(
                binding.stream_id.clone(),
                Arc::new(Reservation {
                    owner: actor.user_id,
                    node: request.node_id,
                    binding,
                    nonce: hash(&nonce),
                    ticket: hash(&ticket),
                    expires: Instant::now() + Duration::from_secs(30),
                    worker_attached: AtomicBool::new(false),
                    viewer_attached: AtomicBool::new(false),
                    to_worker,
                    to_viewer,
                    worker_output: Mutex::new(Some(worker_output)),
                    viewer_output: Mutex::new(Some(viewer_output)),
                    stopped: CancellationToken::new(),
                }),
            );
            Ok(pb::DesktopReservation {
                nonce,
                ticket,
                expires_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| invalid())?
                    .as_secs() as i64
                    + 30,
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
            let entry = self.entry(&request.stream_id).await?;
            if entry.owner != actor.user_id
                || entry.node != request.node_id
                || hash(&request.ticket) != entry.ticket
            {
                return Err(denied());
            }
            self.remove(&entry).await;
            Ok(())
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
        let entry = self.entry(&binding.stream_id).await.map_err(status)?;
        if entry.owner != owner
            || entry.node != auth.node_id
            || entry.binding != binding
            || entry.expires <= Instant::now()
            || hash(&hello.nonce) != entry.nonce
            || entry.worker_attached.swap(true, Ordering::AcqRel)
        {
            return Err(status(denied()));
        }
        {
            let mut entries = self.reservations.lock().await;
            if entry.stopped.is_cancelled() || !entries.contains_key(&binding.stream_id) {
                return Err(status(denied()));
            }
            // The Worker has now validated generation/controller/epoch atomically.
            entries.retain(|id, previous| {
                if id != &binding.stream_id
                    && previous.owner == owner
                    && previous.node == entry.node
                    && previous.binding.viewer_id == binding.viewer_id
                {
                    previous.stopped.cancel();
                    false
                } else {
                    true
                }
            });
        }
        let output = entry
            .worker_output
            .lock()
            .await
            .take()
            .ok_or_else(|| status(denied()))?;
        let input = input.map(|frame| match frame?.body {
            Some(pb::desktop_worker_frame::Body::Chunk(chunk)) => Ok(chunk.data),
            _ => Err(status(invalid())),
        });
        self.forward(Box::pin(input), entry, true, Some(auth), None);
        Ok(Response::new(Box::pin(ReceiverStream::new(output))))
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
        let entry = self.entry(&hello.stream_id).await.map_err(status)?;
        if entry.owner != actor.user_id
            || entry.node != hello.node_id
            || entry.expires <= Instant::now()
            || hash(&hello.ticket) != entry.ticket
            || !entry.worker_attached.load(Ordering::Acquire)
            || entry.viewer_attached.swap(true, Ordering::AcqRel)
        {
            return Err(status(denied()));
        }
        let output = entry
            .viewer_output
            .lock()
            .await
            .take()
            .ok_or_else(|| status(denied()))?;
        let input = input.map(|frame| match frame?.body {
            Some(pb::desktop_viewer_frame::Body::Chunk(chunk)) => Ok(chunk.data),
            _ => Err(status(invalid())),
        });
        self.forward(Box::pin(input), entry, false, None, Some(actor));
        Ok(Response::new(Box::pin(ReceiverStream::new(output))))
    }
}
async fn first<T: Send + 'static>(input: &mut tonic::Streaming<T>) -> Result<T, Status> {
    timeout(Duration::from_secs(5), input.message())
        .await
        .map_err(|_| status(invalid()))??
        .ok_or_else(|| status(invalid()))
}
fn hash(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
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
