//! Existing app-server generation owns bounded outgoing host requests and correlation.
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;
use tokio::sync::{mpsc, oneshot, Semaphore};
use uuid::Uuid;
fn unavailable(message: &'static str) -> TalosError {
    TalosError::new(ErrorCode::ServiceUnavailable, message)
}
type Reply = oneshot::Sender<TalosResult<serde_json::Value>>;
#[derive(Debug)]
pub enum DesktopHostEvent {
    Execute {
        id: String,
        request: Box<BrowserHostRequest>,
    },
    Media {
        id: String,
        frame: DesktopMediaFrame,
    },
    Cancel {
        id: String,
    },
}
/// A single Desktop stdio generation. Pending responses are bounded and one-use.
pub struct DesktopHostBridge {
    tx: mpsc::Sender<DesktopHostEvent>,
    rx: Mutex<Option<mpsc::Receiver<DesktopHostEvent>>>,
    pending: Mutex<HashMap<String, Reply>>,
    capacity: Arc<Semaphore>,
    media_capacity: Arc<Semaphore>,
    closed: AtomicBool,
    active: AtomicBool,
}
impl std::fmt::Debug for DesktopHostBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DesktopHostBridge").finish_non_exhaustive()
    }
}
impl DesktopHostBridge {
    pub fn new() -> Arc<Self> {
        let (tx, rx) = mpsc::channel(32);
        Arc::new(Self {
            tx,
            rx: Mutex::new(Some(rx)),
            pending: Mutex::new(HashMap::new()),
            capacity: Arc::new(Semaphore::new(8)),
            media_capacity: Arc::new(Semaphore::new(2)),
            closed: AtomicBool::new(false),
            active: AtomicBool::new(false),
        })
    }
    pub fn activate(&self) {
        if !self.closed.load(Ordering::Acquire) {
            self.active.store(true, Ordering::Release);
        }
    }
    pub fn take_receiver(&self) -> TalosResult<mpsc::Receiver<DesktopHostEvent>> {
        self.rx
            .lock()
            .map_err(|_| TalosError::internal("Browser host lock failed."))?
            .take()
            .ok_or_else(|| unavailable("Browser host is already attached."))
    }
    pub fn complete(&self, id: &str, response: TalosResult<serde_json::Value>) {
        if let Ok(mut pending) = self.pending.lock() {
            if let Some(reply) = pending.remove(id) {
                let _ = reply.send(response);
            }
        }
    }
    pub async fn media(self: &Arc<Self>, frame: DesktopMediaFrame) -> TalosResult<()> {
        if self.closed.load(Ordering::Acquire) || !self.active.load(Ordering::Acquire) {
            return Err(unavailable("Desktop host is not initialized."));
        }
        let _permit = self
            .media_capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| unavailable("Desktop host capacity exceeded."))?;
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .map_err(|_| TalosError::internal("Desktop host lock failed."))?
            .insert(id.clone(), tx);
        let _cancel = PendingRequest {
            bridge: self.clone(),
            id: id.clone(),
        };
        self.tx
            .try_send(DesktopHostEvent::Media { id, frame })
            .map_err(|_| unavailable("Desktop host is unavailable."))?;
        let value = tokio::time::timeout(Duration::from_secs(2), rx)
            .await
            .map_err(|e| {
                TalosError::from_source(
                    ErrorCode::Timeout,
                    "Desktop viewer stopped consuming media.",
                    e,
                )
            })?
            .map_err(|_| unavailable("Desktop host disconnected."))??;
        serde_json::from_value::<MediaAck>(value)
            .map(|MediaAck::Done| ())
            .map_err(|source| {
                TalosError::internal_with_source("Invalid Desktop media acknowledgement.", source)
            })
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.capacity.close();
        self.media_capacity.close();
        if let Ok(mut pending) = self.pending.lock() {
            for (_, reply) in pending.drain() {
                let _ = reply.send(Err(unavailable("Desktop browser host disconnected.")));
            }
        }
    }
    async fn browser_request(
        self: &Arc<Self>,
        request: BrowserHostRequest,
    ) -> TalosResult<BrowserHostResponse> {
        if self.closed.load(Ordering::Acquire) || !self.active.load(Ordering::Acquire) {
            return Err(unavailable("Desktop browser host disconnected."));
        }
        let _permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| unavailable("Desktop browser request capacity exhausted."))?;
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .map_err(|_| TalosError::internal("Browser host lock failed."))?
            .insert(id.clone(), tx);
        let _cancel = PendingRequest {
            bridge: self.clone(),
            id: id.clone(),
        };
        self.tx
            .try_send(DesktopHostEvent::Execute {
                id,
                request: Box::new(request),
            })
            .map_err(|_| unavailable("Desktop browser host is unavailable."))?;
        let value = tokio::time::timeout(Duration::from_secs(25), rx)
            .await
            .map_err(|e| {
                TalosError::from_source(
                    ErrorCode::Timeout,
                    "Desktop browser operation timed out.",
                    e,
                )
            })?
            .map_err(|_| unavailable("Desktop browser host disconnected."))??;
        serde_json::from_value(value).map_err(|source| {
            TalosError::internal_with_source("Invalid Desktop host response.", source)
        })
    }
}
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum MediaAck {
    Done,
}
impl talos_browser::BrowserHostPort for DesktopHostBridge {
    fn available(&self) -> bool {
        !self.closed.load(Ordering::Acquire) && self.active.load(Ordering::Acquire)
    }
    fn request(
        self: Arc<Self>,
        request: BrowserHostRequest,
    ) -> talos_browser::BrowserFuture<'static, BrowserHostResponse> {
        Box::pin(async move { self.browser_request(request).await })
    }
}
struct PendingRequest {
    bridge: Arc<DesktopHostBridge>,
    id: String,
}
impl Drop for PendingRequest {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.bridge.pending.lock() {
            if pending.remove(&self.id).is_some()
                && self
                    .bridge
                    .tx
                    .try_send(DesktopHostEvent::Cancel {
                        id: self.id.clone(),
                    })
                    .is_err()
            {
                // A full control queue cannot safely admit a following input.
                drop(pending);
                self.bridge.close();
            }
        }
    }
}
#[cfg(test)]
#[path = "desktop_host_tests.rs"]
mod tests;
