use std::sync::atomic::{AtomicU8, Ordering};

use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ServerLifecycleState {
    Running = 0,
    Quiescing = 1,
    Draining = 2,
    Stopped = 3,
}

impl ServerLifecycleState {
    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Running,
            1 => Self::Quiescing,
            2 => Self::Draining,
            3 => Self::Stopped,
            _ => Self::Stopped,
        }
    }

    pub fn is_quiescing_or_later(self) -> bool {
        matches!(self, Self::Quiescing | Self::Draining | Self::Stopped)
    }
}

pub struct ServerLifecycle {
    state: AtomicU8,
    updates_tx: watch::Sender<ServerLifecycleState>,
}

impl Default for ServerLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerLifecycle {
    pub fn new() -> Self {
        let (updates_tx, _updates_rx) = watch::channel(ServerLifecycleState::Running);
        Self {
            state: AtomicU8::new(ServerLifecycleState::Running as u8),
            updates_tx,
        }
    }

    pub fn current(&self) -> ServerLifecycleState {
        ServerLifecycleState::from_u8(self.state.load(Ordering::SeqCst))
    }

    pub fn store(&self, next: ServerLifecycleState) {
        self.state.store(next as u8, Ordering::SeqCst);
        let _ = self.updates_tx.send(next);
    }

    pub fn subscribe(&self) -> watch::Receiver<ServerLifecycleState> {
        self.updates_tx.subscribe()
    }

    pub fn is_quiescing_or_later(&self) -> bool {
        self.current().is_quiescing_or_later()
    }
}
