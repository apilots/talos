//! One bounded, fair writer; per-call queues preserve business ordering.
use std::sync::Arc;
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, StreamExt, StreamMap};
use tokio_util::sync::CancellationToken;

pub(super) const CALL_CAPACITY: usize = 8;
pub(super) const MAX_CALLS: usize = 32;
pub(super) const CONTROL_CALLS: usize = 4;
const CONTROL_CAPACITY: usize = MAX_CALLS + CONTROL_CALLS + 8;
const DELIVERY_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

type Attached<T> = (u64, mpsc::Receiver<T>);

pub(super) struct Writer<T> {
    control: mpsc::Sender<T>,
    attach: mpsc::Sender<Attached<T>>,
    stop: CancellationToken,
}

impl<T: Send + 'static> Writer<T> {
    pub(super) fn new(stop: CancellationToken) -> (Arc<Self>, mpsc::Receiver<T>) {
        let (control, mut controls) = mpsc::channel(CONTROL_CAPACITY);
        let (attach, mut attached) = mpsc::channel::<Attached<T>>(MAX_CALLS + CONTROL_CALLS);
        let (out, inbound) = mpsc::channel(CALL_CAPACITY);
        let writer = Arc::new(Self {
            control,
            attach,
            stop: stop.clone(),
        });
        tokio::spawn(async move {
            let mut calls = StreamMap::new();
            loop {
                let frame = tokio::select! {
                    biased;
                    _ = stop.cancelled() => break,
                    Some(frame) = controls.recv() => frame,
                    Some((id, receiver)) = attached.recv() => {
                        calls.insert(id, ReceiverStream::new(receiver));
                        continue;
                    }
                    Some((_, frame)) = calls.next(), if !calls.is_empty() => frame,
                };
                if tokio::time::timeout(DELIVERY_WAIT, out.send(frame))
                    .await
                    .map_or(true, |sent| sent.is_err())
                {
                    break;
                }
            }
            stop.cancel();
        });
        (writer, inbound)
    }

    pub(super) fn control(&self, frame: T) -> TalosResult<()> {
        self.control.try_send(frame).map_err(|_| {
            self.stop.cancel();
            overloaded()
        })
    }

    pub(super) fn call(&self, id: u64) -> TalosResult<mpsc::Sender<T>> {
        let (sender, receiver) = mpsc::channel(CALL_CAPACITY);
        self.attach
            .try_send((id, receiver))
            .map_err(|_| overloaded())?;
        Ok(sender)
    }
}

pub(super) fn overloaded() -> TalosError {
    TalosError::new(
        ErrorCode::ServiceUnavailable,
        "Agent connection call capacity exceeded",
    )
}

pub(super) fn closed() -> TalosError {
    TalosError::new(
        ErrorCode::ServiceUnavailable,
        "Agent execution connection is unavailable",
    )
}
