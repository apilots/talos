//! One bounded desktop relay for authenticated RPC and trusted in-process ingress.
use agent_client::DesktopReservation;
use agent_protocol::{desktop_pb, AgentFuture};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use talos_browser::DesktopTransport;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{BrowserInputMode, DesktopStreamBinding, WorkerDesktopOpen};
use tokio::{
    sync::{mpsc, Mutex},
    time::timeout,
};
use tokio_util::sync::CancellationToken;

struct Reservation {
    owner: String,
    node: String,
    binding: DesktopStreamBinding,
    nonce: [u8; 32],
    ticket: [u8; 32],
    expires: Instant,
    worker_attached: AtomicBool,
    viewer_attached: AtomicBool,
    to_worker: mpsc::Sender<TalosResult<Vec<u8>>>,
    to_viewer: mpsc::Sender<TalosResult<Vec<u8>>>,
    worker_output: Mutex<Option<mpsc::Receiver<TalosResult<Vec<u8>>>>>,
    viewer_output: Mutex<Option<mpsc::Receiver<TalosResult<Vec<u8>>>>>,
    stopped: CancellationToken,
}

#[derive(Clone)]
pub(crate) struct DesktopRelay {
    reservations: Arc<Mutex<HashMap<String, Arc<Reservation>>>>,
    shutdown: CancellationToken,
}

impl DesktopRelay {
    pub(crate) fn new(shutdown: CancellationToken) -> Self {
        Self {
            reservations: Arc::default(),
            shutdown,
        }
    }

    /// Trusted composition selects a route before validating its nonce. A failed
    /// local admission must never fall through to a different media authority.
    pub(crate) async fn contains_reservation(&self, stream: &str) -> bool {
        self.reservations.lock().await.contains_key(stream)
    }

    pub(crate) async fn reserve(
        &self,
        owner: String,
        node: String,
        binding: DesktopStreamBinding,
    ) -> TalosResult<DesktopReservation> {
        desktop_pb::validate_binding(&binding)?;
        let expires_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| invalid())?
            .as_secs() as i64
            + 30;
        let mut entries = self.reservations.lock().await;
        entries.retain(|_, entry| {
            let keep = !entry.stopped.is_cancelled()
                && (entry.expires > Instant::now()
                    || entry.viewer_attached.load(Ordering::Acquire));
            if !keep {
                entry.stopped.cancel();
            }
            keep
        });
        let node_entries: Vec<_> = entries
            .values()
            .filter(|entry| entry.node == node)
            .collect();
        let same_viewer = node_entries
            .iter()
            .filter(|entry| entry.binding.viewer_id == binding.viewer_id)
            .count();
        let viewers: std::collections::HashSet<_> = node_entries
            .iter()
            .map(|entry| &entry.binding.viewer_id)
            .collect();
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
                owner,
                node,
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
        Ok(DesktopReservation {
            nonce,
            ticket,
            expires_at,
        })
    }

    pub(crate) async fn cancel(
        &self,
        owner: &str,
        node: &str,
        stream: &str,
        ticket: &str,
    ) -> TalosResult<()> {
        let entry = self.entry(stream).await?;
        if entry.owner != owner || entry.node != node || hash(ticket) != entry.ticket {
            return Err(denied());
        }
        self.remove(&entry).await;
        Ok(())
    }

    /// Network adapters must pass their verified lease owner and node identity.
    pub(crate) async fn worker(
        &self,
        owner: &str,
        node: &str,
        request: WorkerDesktopOpen,
    ) -> TalosResult<DesktopTransport> {
        let entry = self.entry(&request.binding.stream_id).await?;
        if entry.owner != owner || entry.node != node {
            return Err(denied());
        }
        self.attach_worker(entry, request).await
    }

    /// Only trusted composition supplies this connector. The nonce binds the
    /// private Worker request to an already authorized immutable reservation.
    pub(crate) async fn attach_local_worker(
        &self,
        request: WorkerDesktopOpen,
    ) -> TalosResult<DesktopTransport> {
        let entry = self.entry(&request.binding.stream_id).await?;
        self.attach_worker(entry, request).await
    }

    async fn attach_worker(
        &self,
        entry: Arc<Reservation>,
        request: WorkerDesktopOpen,
    ) -> TalosResult<DesktopTransport> {
        if entry.binding != request.binding
            || entry.expires <= Instant::now()
            || hash(&request.nonce) != entry.nonce
            || entry.worker_attached.swap(true, Ordering::AcqRel)
        {
            return Err(denied());
        }
        {
            let mut entries = self.reservations.lock().await;
            if entry.stopped.is_cancelled()
                || !entries
                    .get(&entry.binding.stream_id)
                    .is_some_and(|current| Arc::ptr_eq(current, &entry))
            {
                return Err(denied());
            }
            // Admission has been validated by the Browser owner. Only now retire
            // the previous stream for this owner, node and viewer.
            entries.retain(|id, previous| {
                let replace = id != &entry.binding.stream_id
                    && previous.owner == entry.owner
                    && previous.node == entry.node
                    && previous.binding.viewer_id == entry.binding.viewer_id;
                if replace {
                    previous.stopped.cancel();
                }
                !replace
            });
        }
        let output = entry.worker_output.lock().await.take().ok_or_else(denied)?;
        Ok(self.transport(
            entry.clone(),
            output,
            entry.to_viewer.clone(),
            entry.to_worker.clone(),
        ))
    }

    pub(crate) async fn viewer(
        &self,
        owner: &str,
        node: &str,
        stream: &str,
        ticket: &str,
    ) -> TalosResult<DesktopTransport> {
        let entry = self.entry(stream).await?;
        if entry.owner != owner
            || entry.node != node
            || entry.expires <= Instant::now()
            || hash(ticket) != entry.ticket
            || !entry.worker_attached.load(Ordering::Acquire)
            || entry.viewer_attached.swap(true, Ordering::AcqRel)
        {
            return Err(denied());
        }
        let output = entry.viewer_output.lock().await.take().ok_or_else(denied)?;
        Ok(self.transport(
            entry.clone(),
            output,
            entry.to_worker.clone(),
            entry.to_viewer.clone(),
        ))
    }

    async fn entry(&self, stream: &str) -> TalosResult<Arc<Reservation>> {
        self.reservations
            .lock()
            .await
            .get(stream)
            .filter(|entry| !entry.stopped.is_cancelled())
            .cloned()
            .ok_or_else(denied)
    }

    async fn remove(&self, entry: &Arc<Reservation>) {
        entry.stopped.cancel();
        let mut entries = self.reservations.lock().await;
        if entries
            .get(&entry.binding.stream_id)
            .is_some_and(|current| Arc::ptr_eq(current, entry))
        {
            entries.remove(&entry.binding.stream_id);
        }
    }

    fn transport(
        &self,
        entry: Arc<Reservation>,
        incoming: mpsc::Receiver<TalosResult<Vec<u8>>>,
        peer: mpsc::Sender<TalosResult<Vec<u8>>>,
        receiving: mpsc::Sender<TalosResult<Vec<u8>>>,
    ) -> DesktopTransport {
        let (outgoing, mut input) = mpsc::channel::<Vec<u8>>(64);
        let stopped = entry.stopped.clone();
        let relay = self.clone();
        tokio::spawn(async move {
            let mut check = tokio::time::interval(Duration::from_secs(1));
            let result = async {
                loop {
                    tokio::select! {
                        biased;
                        _=entry.stopped.cancelled()=>break,
                        _=relay.shutdown.cancelled()=>break,
                        _=receiving.closed()=>break,
                        _=check.tick()=>{
                            if entry.expires<=Instant::now() && !entry.viewer_attached.load(Ordering::Acquire) {return Err(denied());}
                        },
                        data=input.recv()=>{
                            let Some(data)=data else {break;};
                            if data.is_empty() || data.len()>64*1024 {return Err(invalid());}
                            timeout(Duration::from_secs(2), peer.send(Ok(data))).await
                                .map_err(|error|TalosError::from_source(ErrorCode::Timeout,"Desktop consumer exceeded its bounded queue.",error))?
                                .map_err(|_|unavailable())?;
                        },
                    }
                }
                Ok::<_,TalosError>(())
            }.await;
            if let Err(error) = result {
                talos_logging::report_error(&error);
                let _ = peer.try_send(Err(error));
            }
            relay.remove(&entry).await;
        });
        DesktopTransport {
            incoming,
            outgoing,
            stopped,
        }
    }
}

/// Adapters provide their existing identity/lease check. No bytes or extra
/// queues pass through this watchdog; cancelling the shared token closes both ends.
pub(crate) fn guard_transport(
    transport: DesktopTransport,
    authorize: Arc<dyn Fn() -> AgentFuture<'static, TalosResult<()>> + Send + Sync>,
) -> DesktopTransport {
    let stopped = transport.stopped.clone();
    tokio::spawn(async move {
        let mut check = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                biased;
                _=stopped.cancelled()=>return,
                _=check.tick()=>{},
            }
            let result =
                tokio::select! { _=stopped.cancelled()=>return,result=authorize()=>result };
            if let Err(error) = result {
                talos_logging::report_error(&error);
                stopped.cancel();
                return;
            }
        }
    });
    transport
}

fn hash(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
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

#[cfg(test)]
#[path = "desktop_relay_tests.rs"]
mod tests;
