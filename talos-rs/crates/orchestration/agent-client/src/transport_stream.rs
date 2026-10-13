use futures_util::{Stream, StreamExt};
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
};
use talos_error::TalosError;
use tokio_util::sync::CancellationToken;
use tonic::Status;

pub(crate) type CallFailure = (CancellationToken, Arc<Mutex<Option<TalosError>>>);
pub(crate) struct RpcStream<T> {
    pub(crate) stream: Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>,
    pub(crate) failure: Option<CallFailure>,
}
impl<T: Send + 'static> RpcStream<T> {
    pub(crate) fn new(stream: impl Stream<Item = Result<T, Status>> + Send + 'static) -> Self {
        Self {
            stream: Box::pin(stream),
            failure: None,
        }
    }
    pub(crate) async fn message(&mut self) -> Result<Option<T>, Status> {
        self.stream.next().await.transpose()
    }
    pub(crate) fn failed(&self) -> impl std::future::Future<Output = TalosError> + Send + 'static {
        let failure = self.failure.clone();
        async move {
            if let Some((cancel, error)) = failure {
                cancel.cancelled().await;
                return error
                    .lock()
                    .expect("Agent call lock poisoned")
                    .clone()
                    .unwrap_or_else(super::connection_writer::closed);
            }
            futures_util::future::pending().await
        }
    }
    pub(crate) fn map<U: Send + 'static>(
        self,
        convert: impl Fn(T) -> Result<U, Status> + Send + 'static,
    ) -> RpcStream<U> {
        RpcStream {
            stream: Box::pin(self.stream.map(move |result| result.and_then(&convert))),
            failure: self.failure,
        }
    }
}
