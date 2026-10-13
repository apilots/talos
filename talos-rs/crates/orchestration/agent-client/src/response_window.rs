//! Per-call credits keep ordinary streaming bursts bounded without blocking other calls.
use super::connection_writer::{overloaded, CALL_CAPACITY};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::sync::Semaphore;

pub(super) struct ResponseWindow {
    permits: Semaphore,
    sent: AtomicU64,
    acknowledged: AtomicU64,
}
impl ResponseWindow {
    pub(super) fn new(size: u32) -> TalosResult<Arc<Self>> {
        if size as usize != CALL_CAPACITY {
            return Err(invalid());
        }
        Ok(Arc::new(Self {
            permits: Semaphore::new(CALL_CAPACITY),
            sent: AtomicU64::new(0),
            acknowledged: AtomicU64::new(0),
        }))
    }
    pub(super) async fn reserve(&self) -> TalosResult<()> {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.permits.acquire())
            .await
            .map_err(|_| {
                overloaded().context("Agent response consumer did not release its receive window")
            })?
            .map_err(|_| invalid())?
            .forget();
        Ok(())
    }
    pub(super) fn sent(&self, sequence: u64) {
        self.sent.store(sequence, Ordering::Release);
    }
    pub(super) fn acknowledge(&self, sequence: u64) -> TalosResult<()> {
        let previous = self.acknowledged.load(Ordering::Acquire);
        let sent = self.sent.load(Ordering::Acquire);
        if sequence <= previous || sequence > sent || sequence - previous > CALL_CAPACITY as u64 {
            return Err(invalid().context(format!(
                "Receive window: acknowledgement={sequence}, previous={previous}, sent={sent}"
            )));
        }
        self.acknowledged.store(sequence, Ordering::Release);
        self.permits.add_permits((sequence - previous) as usize);
        Ok(())
    }
}
fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Agent response acknowledgement is invalid",
    )
}
