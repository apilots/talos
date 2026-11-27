use axum::body::Bytes;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use talos_app_protocol::relay::{
    ReceiveWindow, RelayLane, RelayResponseHead, RelayToHost, SendWindow, RELAY_WINDOW_FRAMES,
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::sync::{mpsc, oneshot, Mutex, Notify};
use tokio_util::sync::CancellationToken;

pub(crate) enum Chunk {
    Data { sequence: u64, bytes: Bytes },
    Failure(TalosError),
}
pub(crate) struct Incoming {
    pub(crate) window: ReceiveWindow,
    pub(crate) sender: Option<mpsc::Sender<Chunk>>,
}
pub(crate) struct Stream {
    pub(crate) head: Mutex<Option<oneshot::Sender<TalosResult<RelayResponseHead>>>>,
    pub(crate) incoming: Mutex<Incoming>,
    pub(crate) outgoing: Mutex<SendWindow>,
    pub(crate) credit: Notify,
    pub(crate) cancel: CancellationToken,
    pub(crate) started: AtomicBool,
}
impl Stream {
    pub(crate) fn new() -> (
        Arc<Self>,
        oneshot::Receiver<TalosResult<RelayResponseHead>>,
        mpsc::Receiver<Chunk>,
    ) {
        let (head_tx, head_rx) = oneshot::channel();
        let (sender, body) = mpsc::channel(RELAY_WINDOW_FRAMES as usize + 1);
        (
            Arc::new(Self {
                head: Mutex::new(Some(head_tx)),
                incoming: Mutex::new(Incoming {
                    window: Default::default(),
                    sender: Some(sender),
                }),
                outgoing: Mutex::new(Default::default()),
                credit: Notify::new(),
                cancel: CancellationToken::new(),
                started: AtomicBool::new(false),
            }),
            head_rx,
            body,
        )
    }
    pub(crate) async fn fail(&self, error: TalosError) {
        if let Some(sender) = self.head.lock().await.take() {
            let _ = sender.send(Err(error.clone()));
        }
        if let Some(sender) = self.incoming.lock().await.sender.take() {
            let _ = sender.try_send(Chunk::Failure(error));
        }
        self.cancel.cancel();
        self.credit.notify_waiters();
    }
    pub(crate) async fn reserve(&self, connection: &CancellationToken) -> TalosResult<u64> {
        loop {
            let notified = self.credit.notified();
            if self.cancel.is_cancelled() || connection.is_cancelled() {
                return Err(unavailable());
            }
            if let Some(sequence) = self.outgoing.lock().await.reserve() {
                return Ok(sequence);
            }
            tokio::select! {_=connection.cancelled()=>return Err(unavailable()),_=self.cancel.cancelled()=>return Err(unavailable()),_=notified=>{}}
        }
    }
    pub(crate) fn started(&self) -> bool {
        self.started.load(Ordering::Acquire)
    }
}

#[derive(Clone)]
pub(crate) struct Outbox {
    senders: [mpsc::Sender<RelayToHost>; 4],
    cancel: CancellationToken,
}
pub(crate) struct Receivers {
    pub(crate) lanes: [mpsc::Receiver<RelayToHost>; 4],
}
impl Outbox {
    pub(crate) fn new(cancel: CancellationToken) -> (Self, Receivers) {
        let (c, cr) = mpsc::channel(1024);
        let (i, ir) = mpsc::channel(32);
        let (f, fr) = mpsc::channel(32);
        let (m, mr) = mpsc::channel(32);
        (
            Self {
                senders: [c, i, f, m],
                cancel,
            },
            Receivers {
                lanes: [cr, ir, fr, mr],
            },
        )
    }
    pub(crate) async fn send(&self, lane: RelayLane, frame: RelayToHost) -> TalosResult<()> {
        frame.validate()?;
        tokio::select! {_=self.cancel.cancelled()=>Err(unavailable()),r=self.senders[lane.index()].send(frame)=>r.map_err(|_|unavailable())}
    }
    pub(crate) fn control(&self, frame: RelayToHost) -> TalosResult<()> {
        frame.validate()?;
        self.senders[0].try_send(frame).map_err(|_| {
            self.cancel.cancel();
            unavailable()
        })
    }
}
impl Receivers {
    pub(crate) async fn next(&mut self, position: &mut usize) -> Option<RelayToHost> {
        if let Ok(frame) = self.lanes[0].try_recv() {
            return Some(frame);
        }
        for _ in 0..3 {
            *position = (*position % 3) + 1;
            if let Ok(frame) = self.lanes[*position].try_recv() {
                return Some(frame);
            }
        }
        let [control, interactive, file, media] = &mut self.lanes;
        tokio::select! {biased; x=control.recv()=>x,x=interactive.recv()=>x,x=file.recv()=>x,x=media.recv()=>x}
    }
}

pub(crate) struct Connection {
    pub(crate) id: uuid::Uuid,
    pub(crate) auth: agent_protocol::NodeAuth,
    pub(crate) host_id: String,
    pub(crate) streams: Mutex<HashMap<u64, Arc<Stream>>>,
    pub(crate) outgoing: Outbox,
    pub(crate) cancel: CancellationToken,
}
impl Connection {
    pub(crate) fn new(auth: agent_protocol::NodeAuth, host_id: String) -> (Arc<Self>, Receivers) {
        let cancel = CancellationToken::new();
        let (outgoing, receivers) = Outbox::new(cancel.clone());
        (
            Arc::new(Self {
                id: uuid::Uuid::new_v4(),
                auth,
                host_id,
                streams: Default::default(),
                outgoing,
                cancel,
            }),
            receivers,
        )
    }
    pub(crate) async fn fail_all(&self) {
        self.cancel.cancel();
        let streams = std::mem::take(&mut *self.streams.lock().await);
        for stream in streams.into_values() {
            stream.fail(unavailable()).await;
        }
    }
}
pub(crate) fn unavailable() -> TalosError {
    TalosError::new(
        ErrorCode::ServiceUnavailable,
        "Personal computer product connection is unavailable.",
    )
}

pub(crate) struct StreamGuard {
    pub(crate) stream: Arc<Stream>,
}
impl Drop for StreamGuard {
    fn drop(&mut self) {
        self.stream.cancel.cancel();
    }
}
