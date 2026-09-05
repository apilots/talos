use std::future::Future;

use tokio_util::sync::CancellationToken;

/// One-way cancellation signal for a single run.
#[derive(Clone)]
pub struct Cancellation {
    requested: CancellationToken,
}

impl Cancellation {
    pub(crate) fn new() -> Self {
        Self {
            requested: CancellationToken::new(),
        }
    }

    pub(crate) fn request(&self) {
        self.requested.cancel();
    }

    /// Return whether cooperative cancellation has already been requested.
    pub fn is_requested(&self) -> bool {
        self.requested.is_cancelled()
    }

    /// Wait until cooperative cancellation is requested.
    pub fn requested(&self) -> impl Future<Output = ()> + Send + 'static {
        let requested = self.requested.clone();
        async move { requested.cancelled().await }
    }
}
