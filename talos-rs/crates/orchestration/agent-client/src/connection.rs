//! Control-side live invocation channels. No business state or replay is owned here.
use super::connection_writer::{
    closed, overloaded, Writer, CALL_CAPACITY, CONTROL_CALLS, MAX_CALLS,
};
use agent_pb as pb;
use agent_protocol::{error_pb, node_pb, NodeAuth};
use futures_util::{Stream, StreamExt};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tonic::Status;

type Reply = pb::worker_rpc_result::Response;
type Call = pb::worker_rpc_call::Request;
use super::transport_stream::RpcStream;

#[derive(Clone, Copy)]
pub(crate) enum RequestMode {
    Unary,
    Streaming,
    ControlUnary,
    ControlStreaming,
}

struct Pending {
    sequence: u64,
    scope: Option<pb::CoordinatorCallContext>,
    sender: mpsc::Sender<Reply>,
    cancel: CancellationToken,
    error: Arc<Mutex<Option<TalosError>>>,
}

/// The accepted node connection, shared by all advertised Harness clients.
/// Losing this handle cancels invocation channels; the publisher owns Registry removal.
pub struct AgentExecutionConnection {
    writer: Arc<Writer<pb::ControlToNodeFrame>>,
    calls: Mutex<HashMap<u64, Pending>>,
    next_id: AtomicU64,
    stop: CancellationToken,
    auth: NodeAuth,
}

impl AgentExecutionConnection {
    /// Accept an already authenticated lease stream. The ingress owns lease validation
    /// and serializes Registry installation and replacement before acknowledging it.
    pub fn accept<S>(
        auth: NodeAuth,
        inbound: S,
    ) -> (Arc<Self>, mpsc::Receiver<pb::ControlToNodeFrame>)
    where
        S: Stream<Item = Result<pb::NodeToControlFrame, Status>> + Send + 'static,
    {
        let mut inbound = Box::pin(inbound);
        let stop = CancellationToken::new();
        let (writer, output) = Writer::new(stop.clone());
        let connection = Arc::new(Self {
            writer,
            calls: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            stop,
            auth,
        });
        let reader = connection.clone();
        tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    biased;
                    _ = reader.stop.cancelled() => break,
                    frame = inbound.next() => frame,
                };
                match frame {
                    Some(Ok(pb::NodeToControlFrame {
                        body: Some(pb::node_to_control_frame::Body::Result(result)),
                    })) => {
                        if reader.deliver(result).is_err() {
                            break;
                        }
                        // Let invocation consumers run between buffered network frames.
                        tokio::task::yield_now().await;
                    }
                    _ => break,
                }
            }
            reader.close();
        });
        (connection, output)
    }

    /// Acknowledge only after all advertised Harnesses have been installed.
    pub fn ready(&self) -> TalosResult<()> {
        self.writer.control(pb::ControlToNodeFrame {
            body: Some(pb::control_to_node_frame::Body::Connected(
                pb::ConnectNodeResponse {
                    result: Some(pb::connect_node_response::Result::Ready(pb::Empty {})),
                },
            )),
        })
    }

    /// Wait for loss, revocation or explicit close so the publisher can withdraw this handle.
    pub async fn ended(&self) {
        self.stop.cancelled().await;
    }

    pub fn is_closed(&self) -> bool {
        self.stop.is_cancelled()
    }

    /// Stop admission and fail pending calls; never replay calls or persist execution state.
    pub fn close(&self) {
        self.stop.cancel();
        let calls =
            std::mem::take(&mut *self.calls.lock().expect("Agent connection lock poisoned"));
        for (_, pending) in calls {
            fail(pending, closed());
        }
    }

    fn deliver(&self, result: pb::WorkerRpcResult) -> TalosResult<()> {
        let mut calls = self.calls.lock().expect("Agent connection lock poisoned");
        let Some(pending) = calls.get_mut(&result.call_id) else {
            return Ok(());
        };
        if result.sequence != pending.sequence || (!result.completed && result.response.is_none()) {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Agent result sequence is invalid",
            ));
        }
        pending.sequence += 1;
        let error = match result.response {
            Some(Reply::Failure(value)) => Some(error_pb::decode_failure(*value)?),
            Some(value) => pending.sender.try_send(value).err().map(|_| overloaded()),
            None => None,
        };
        if let Some(error) = error {
            let pending = calls.remove(&result.call_id).expect("pending call");
            drop(calls);
            fail(pending, error);
            self.cancel(result.call_id);
        } else if result.completed {
            calls.remove(&result.call_id);
        }
        Ok(())
    }

    fn cancel(&self, id: u64) {
        let _ = self.writer.control(pb::ControlToNodeFrame {
            body: Some(pb::control_to_node_frame::Body::Cancel(
                pb::CancelWorkerRpcCall {
                    auth: Some(node_pb::encode_auth(self.auth.clone())),
                    call_id: id,
                },
            )),
        });
    }

    pub(crate) async fn invoke<S>(
        self: &Arc<Self>,
        requests: S,
        timeout_ms: u64,
        mode: RequestMode,
    ) -> Result<RpcStream<Reply>, Status>
    where
        S: Stream<Item = Call> + Send + 'static,
    {
        self.invoke_results(requests.map(Ok), timeout_ms, mode, None)
            .await
    }

    async fn invoke_results<S>(
        self: &Arc<Self>,
        requests: S,
        timeout_ms: u64,
        mode: RequestMode,
        caller: Option<pb::CoordinatorCallContext>,
    ) -> Result<RpcStream<Reply>, Status>
    where
        S: Stream<Item = Result<Call, Status>> + Send + 'static,
    {
        if self.stop.is_cancelled() {
            return Err(error_pb::encode_status(&closed()));
        }
        let deadline = if timeout_ms == 0 {
            None
        } else {
            Some(
                tokio::time::Instant::now()
                    .checked_add(std::time::Duration::from_millis(timeout_ms))
                    .ok_or_else(|| {
                        error_pb::encode_status(&TalosError::new(
                            ErrorCode::InvalidRequest,
                            "Agent RPC timeout is invalid",
                        ))
                    })?,
            )
        };
        let mut requests = Box::pin(requests);
        let first = requests
            .next()
            .await
            .transpose()?
            .ok_or_else(|| error_pb::encode_status(&closed()))?;
        let scope = agent_protocol::connection_pb::execution_scope(&first);
        let requests_closed = matches!(mode, RequestMode::Unary | RequestMode::ControlUnary);
        let id;
        let cancel = CancellationToken::new();
        let error = Arc::new(Mutex::new(None));
        let (sender, receiver) = mpsc::channel(CALL_CAPACITY);
        {
            let mut calls = self.calls.lock().expect("Agent connection lock poisoned");
            let capacity = MAX_CALLS
                + if matches!(
                    mode,
                    RequestMode::ControlUnary | RequestMode::ControlStreaming
                ) {
                    CONTROL_CALLS
                } else {
                    0
                };
            if calls.len() >= capacity {
                return Err(error_pb::encode_status(&overloaded()));
            }
            id = self.next_id.fetch_add(1, Ordering::Relaxed);
            if id == 0 {
                self.stop.cancel();
                return Err(error_pb::encode_status(&closed()));
            }
            self.writer
                .control(pb::ControlToNodeFrame {
                    body: Some(pb::control_to_node_frame::Body::Call(pb::WorkerRpcCall {
                        caller: caller.clone(),
                        call_id: id,
                        sequence: 1,
                        requests_closed,
                        timeout_ms,
                        response_window: CALL_CAPACITY as u32,
                        request: Some(first),
                    })),
                })
                .map_err(|error| error_pb::encode_status(&error))?;
            calls.insert(
                id,
                Pending {
                    sequence: 1,
                    scope,
                    sender,
                    cancel: cancel.clone(),
                    error: error.clone(),
                },
            );
        }
        if let Some(deadline) = deadline {
            let connection = self.clone();
            let completed = cancel.clone();
            tokio::spawn(async move {
                tokio::select! {
                    biased;
                    _ = completed.cancelled() => {}
                    _ = connection.stop.cancelled() => {}
                    _ = tokio::time::sleep_until(deadline) => {
                        let pending = connection.calls.lock().expect("Agent connection lock poisoned").remove(&id);
                        if let Some(pending) = pending {
                            fail(pending, TalosError::new(ErrorCode::Timeout, "Agent RPC deadline exceeded"));
                            connection.cancel(id);
                        }
                    }
                }
            });
        }
        let acknowledgements = self.writer.call(id).map_err(|error| {
            self.calls
                .lock()
                .expect("Agent connection lock poisoned")
                .remove(&id);
            self.cancel(id);
            error_pb::encode_status(&error)
        })?;
        if !requests_closed {
            let out = acknowledgements.clone();
            let connection = self.clone();
            let send_stop = cancel.clone();
            tokio::spawn(async move {
                let mut sequence = 2;
                loop {
                    let request = tokio::select! {
                        biased;
                        _ = connection.stop.cancelled() => break,
                        _ = send_stop.cancelled() => break,
                        request = requests.next() => request,
                    };
                    let request = match request {
                        Some(Ok(request)) => Some(request),
                        Some(Err(error)) => {
                            if let Some(pending) = connection
                                .calls
                                .lock()
                                .expect("Agent connection lock poisoned")
                                .remove(&id)
                            {
                                fail(pending, crate::decode_rpc_status(error));
                            }
                            connection.cancel(id);
                            break;
                        }
                        None => None,
                    };
                    let requests_closed = request.is_none();
                    let frame = pb::ControlToNodeFrame {
                        body: Some(pb::control_to_node_frame::Body::Call(pb::WorkerRpcCall {
                            caller: None,
                            call_id: id,
                            sequence,
                            requests_closed,
                            timeout_ms: 0,
                            response_window: 0,
                            request,
                        })),
                    };
                    let sent = tokio::select! {
                        biased;
                        _ = send_stop.cancelled() => break,
                        _ = connection.stop.cancelled() => break,
                        result = tokio::time::timeout(std::time::Duration::from_secs(5), out.send(frame)) => result,
                    };
                    if sent.map_or(true, |result| result.is_err()) {
                        if let Some(pending) = connection
                            .calls
                            .lock()
                            .expect("Agent connection lock poisoned")
                            .remove(&id)
                        {
                            fail(pending, overloaded());
                        }
                        connection.cancel(id);
                        break;
                    }
                    sequence += 1;
                    if requests_closed {
                        break;
                    }
                }
            });
        }
        let guard = CallGuard {
            connection: self.clone(),
            id,
            cancel: cancel.clone(),
        };
        let failure = Some((cancel.clone(), error.clone()));
        Ok(RpcStream {
            stream: Box::pin(futures_util::stream::unfold(
                (
                    receiver,
                    cancel,
                    error,
                    Some(guard),
                    acknowledgements,
                    1_u64,
                ),
                move |(mut receiver, cancel, error, guard, acknowledgements, sequence)| async move {
                    let item = tokio::select! {
                        biased;
                        _ = cancel.cancelled() => {
                            let error = error.lock().expect("Agent call lock poisoned").take();
                            error.map(|error| Err(error_pb::encode_status(&error)))
                        }
                        item = receiver.recv() => item.map(Ok),
                    };
                    let active = guard
                        .as_ref()
                        .expect("call guard")
                        .connection
                        .calls
                        .lock()
                        .expect("Agent connection lock poisoned")
                        .contains_key(&id);
                    if matches!(item, Some(Ok(_))) && active {
                        let frame = pb::ControlToNodeFrame {
                            body: Some(pb::control_to_node_frame::Body::Acknowledge(
                                pb::AcknowledgeWorkerRpcResult {
                                    auth: Some(node_pb::encode_auth(
                                        guard.as_ref().expect("call guard").connection.auth.clone(),
                                    )),
                                    call_id: id,
                                    sequence,
                                },
                            )),
                        };
                        let delivered = tokio::select! {
                            biased;
                            _ = cancel.cancelled() => false,
                            result = tokio::time::timeout(std::time::Duration::from_secs(5), acknowledgements.send(frame)) => matches!(result, Ok(Ok(()))),
                        };
                        if !delivered {
                            return Some((
                                Err(error_pb::encode_status(&closed())),
                                (receiver, cancel, error, guard, acknowledgements, sequence),
                            ));
                        }
                    }
                    item.map(|item| {
                        (
                            item,
                            (
                                receiver,
                                cancel,
                                error,
                                guard,
                                acknowledgements,
                                sequence + 1,
                            ),
                        )
                    })
                },
            )),
            failure,
        })
    }
}

struct CallGuard {
    connection: Arc<AgentExecutionConnection>,
    id: u64,
    cancel: CancellationToken,
}
impl Drop for CallGuard {
    fn drop(&mut self) {
        self.cancel.cancel();
        if self
            .connection
            .calls
            .lock()
            .expect("Agent connection lock poisoned")
            .remove(&self.id)
            .is_some()
        {
            self.connection.cancel(self.id);
        }
    }
}

fn fail(pending: Pending, error: TalosError) {
    *pending.error.lock().expect("Agent call lock poisoned") = Some(error);
    pending.cancel.cancel();
}

#[path = "connection_relay.rs"]
mod relay;

#[path = "resource_connection.rs"]
mod resources;
