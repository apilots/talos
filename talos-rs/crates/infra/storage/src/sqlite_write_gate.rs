use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use tokio::sync::{Mutex, Notify, OwnedMutexGuard};

#[derive(Default)]
pub(super) struct WriteGate {
    writer: Arc<Mutex<()>>,
    foreground: AtomicUsize,
    foreground_drained: Notify,
}

impl WriteGate {
    pub(super) async fn foreground(&self) -> OwnedMutexGuard<()> {
        self.foreground.fetch_add(1, Ordering::SeqCst);
        let _pending = ForegroundWait(self);
        self.writer.clone().lock_owned().await
    }

    pub(super) async fn background(&self) -> OwnedMutexGuard<()> {
        loop {
            let drained = self.foreground_drained.notified();
            let writer = self.writer.clone().lock_owned().await;
            if self.foreground.load(Ordering::SeqCst) == 0 {
                return writer;
            }
            drop(writer);
            drained.await;
        }
    }
}

struct ForegroundWait<'a>(&'a WriteGate);

impl Drop for ForegroundWait<'_> {
    fn drop(&mut self) {
        if self.0.foreground.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.0.foreground_drained.notify_waiters();
        }
    }
}
