//! Worker-side carrier. All requests execute through the existing Harness adapter.
use super::response_window::ResponseWindow;
use super::{
    connection_writer::{closed, overloaded, Writer, CALL_CAPACITY, CONTROL_CALLS, MAX_CALLS},
    AgentNodeClient,
};
use agent_pb as pb;
use agent_protocol::{error_pb, node_pb, NodeAuth};
use futures_util::StreamExt;
use std::sync::Arc;
use std::{collections::HashMap, time::Duration};
use talos_agent::AgentHarnessRpcAdapter;
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::{
    sync::{mpsc, watch},
    task::JoinSet,
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

pub(crate) const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

type ControlStream = std::pin::Pin<
    Box<dyn futures_util::Stream<Item = Result<pb::ControlToNodeFrame, tonic::Status>> + Send>,
>;

type Call = pb::worker_rpc_call::Request;
struct Pending {
    sequence: u64,
    response_window: Arc<ResponseWindow>,
    sender: Option<mpsc::Sender<Result<Call, tonic::Status>>>,
    cancel: CancellationToken,
    failure: watch::Sender<Option<TalosError>>,
}

struct Invocation {
    id: u64,
    timeout_ms: u64,
    cancel: CancellationToken,
    failed: watch::Receiver<Option<TalosError>>,
    window: Arc<ResponseWindow>,
}

impl AgentNodeClient {
    /// Connect is one outgoing stream, with no inbound Worker listener or product persistence.
    /// The caller installs the current adapter lease and drains old execution before reconnecting.
    pub async fn run_execution_connection(
        &self,
        auth: NodeAuth,
        adapter: AgentHarnessRpcAdapter,
        stop: CancellationToken,
    ) -> TalosResult<()> {
        let (writer, output) = Writer::new(stop.clone());
        writer.control(pb::NodeToControlFrame {
            body: Some(pb::node_to_control_frame::Body::Connect(
                pb::ConnectNodeRequest {
                    auth: Some(node_pb::encode_auth(auth)),
                    params: Some(pb::Empty {}),
                    stream_heartbeat: true,
                },
            )),
        })?;
        let mut input: ControlStream = if self.uses_websocket() {
            self.websocket_execution_stream(output, stop.clone())
                .await?
        } else {
            Box::pin(
                tokio::time::timeout(
                    Duration::from_secs(10),
                    pb::agent_node_control_client::AgentNodeControlClient::new(
                        self.channel.clone(),
                    )
                    .connect(ReceiverStream::new(output)),
                )
                .await
                .map_err(|error| {
                    TalosError::from_source(ErrorCode::Timeout, "Agent Connect timed out", error)
                })?
                .map_err(crate::decode_rpc_status)?
                .into_inner(),
            )
        };
        let first = tokio::time::timeout(Duration::from_secs(10), input.next())
            .await
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::Timeout,
                    "Agent Connect acknowledgement timed out",
                    error,
                )
            })?
            .ok_or_else(closed)?
            .map_err(crate::decode_rpc_status)?;
        let stream_heartbeat = match first.body {
            Some(pb::control_to_node_frame::Body::Connected(pb::ConnectNodeResponse {
                result: Some(pb::connect_node_response::Result::Ready(_)),
                stream_heartbeat,
            })) => stream_heartbeat,
            Some(pb::control_to_node_frame::Body::Connected(pb::ConnectNodeResponse {
                result: Some(pb::connect_node_response::Result::Failure(error)),
                ..
            })) => return Err(error_pb::decode_failure(error)?),
            _ => return Err(invalid_frame()),
        };
        serve_calls(
            Box::pin(input),
            writer,
            stop,
            Dispatcher(adapter),
            stream_heartbeat,
        )
        .await
    }
    async fn websocket_execution_stream(
        &self,
        mut output: mpsc::Receiver<pb::NodeToControlFrame>,
        stop: CancellationToken,
    ) -> TalosResult<ControlStream> {
        use futures_util::SinkExt;
        use prost::Message;
        let (mut sink, input) = self.open_websocket("connect").await?.split();
        let writer_stop = stop.clone();
        tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    _ = writer_stop.cancelled() => break,
                    frame = output.recv() => match frame { Some(frame) => frame, None => break },
                };
                if frame.encoded_len() > MAX_FRAME_BYTES {
                    break;
                }
                let send = sink.send(reqwest_websocket::Message::Binary(
                    frame.encode_to_vec().into(),
                ));
                if tokio::select! { _ = writer_stop.cancelled() => true, result = send => result.is_err() }
                {
                    break;
                }
            }
            writer_stop.cancel();
        });
        Ok(Box::pin(input.filter_map(move |message| async move {
            match message {
                Ok(reqwest_websocket::Message::Binary(bytes)) => {
                    Some(pb::ControlToNodeFrame::decode(bytes).map_err(|error| {
                        error_pb::encode_status(&TalosError::internal_with_source(
                            "Agent WebSocket frame is malformed",
                            error,
                        ))
                    }))
                }
                Ok(reqwest_websocket::Message::Ping(_) | reqwest_websocket::Message::Pong(_)) => {
                    None
                }
                Ok(_) => Some(Err(error_pb::encode_status(&closed()))),
                Err(error) => Some(Err(error_pb::encode_status(&super::node::websocket_error(
                    error,
                )))),
            }
        })))
    }
}

pub(crate) struct Dispatcher(AgentHarnessRpcAdapter);
impl Dispatcher {
    fn authorize(&self, supplied: &NodeAuth) -> TalosResult<()> {
        self.0.validate_connection_auth(supplied)
    }
    async fn dispatch(
        &self,
        requests: super::transport_stream::RpcStream<Call>,
    ) -> Result<super::transport_stream::RpcStream<pb::worker_rpc_result::Response>, tonic::Status>
    {
        Ok(super::transport_stream::RpcStream::new(
            self.0.dispatch_connect(requests.stream).await?,
        ))
    }
}

pub(crate) async fn serve_calls(
    mut input: std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<pb::ControlToNodeFrame, tonic::Status>> + Send>,
    >,
    writer: Arc<Writer<pb::NodeToControlFrame>>,
    stop: CancellationToken,
    dispatcher: Dispatcher,
    stream_heartbeat: bool,
) -> TalosResult<()> {
    let dispatcher = Arc::new(dispatcher);
    let mut calls = HashMap::<u64, Pending>::new();
    let mut newest_call = 0;
    let mut tasks = JoinSet::new();
    let result = async {
            loop {
                let frame = tokio::select! {
                    biased;
                    _ = stop.cancelled() => return Ok(()),
                    Some(completed) = tasks.join_next(), if !tasks.is_empty() => {
                        let (id, result) = completed.map_err(|error| TalosError::internal_with_source("Agent invocation task failed; technical details were withheld.", error))?;
                        calls.remove(&id);
                        result?;
                        continue;
                    }
                    frame = input.next() => frame.ok_or_else(closed)?.map_err(crate::decode_rpc_status)?,
                };
                match frame.body {
                    Some(pb::control_to_node_frame::Body::Ping(nonce)) if stream_heartbeat => {
                        writer.control(pb::NodeToControlFrame { body: Some(pb::node_to_control_frame::Body::Pong(nonce)) })?;
                    }
                    Some(pb::control_to_node_frame::Body::Acknowledge(acknowledgement)) => {
                        dispatcher.authorize(&node_pb::decode_auth(acknowledgement.auth)?)?;
                        if let Some(pending) = calls.get(&acknowledgement.call_id) { pending.response_window.acknowledge(acknowledgement.sequence)?; }
                    }
                    Some(pb::control_to_node_frame::Body::Cancel(cancel)) => {
                        dispatcher.authorize(&node_pb::decode_auth(cancel.auth)?)?;
                        if let Some(pending) = calls.remove(&cancel.call_id) { pending.cancel.cancel(); }
                    }
                    Some(pb::control_to_node_frame::Body::Call(frame)) => {
                        if frame.call_id == 0 || frame.sequence == 0 { return Err(invalid_frame()); }
                        if !calls.contains_key(&frame.call_id) {
                            if frame.sequence != 1 { continue; }
                            if frame.call_id <= newest_call { return Err(invalid_frame()); }
                            if frame.request.is_none() { return Err(invalid_frame()); }
                            newest_call = frame.call_id;
                            let control = matches!(&frame.request, Some(Call::Interrupt(_) | Call::Shutdown(_) | Call::SubmitInput(_)));
                            if calls.len() >= MAX_CALLS + if control { CONTROL_CALLS } else { 0 } {
                                send_failure(&writer, frame.call_id, 1, overloaded())?;
                                continue;
                            }
                            let response_window = ResponseWindow::new(frame.response_window)?;
                            let (sender, receiver) = mpsc::channel(CALL_CAPACITY);
                            let cancel = stop.child_token();
                            let (failure, failed) = watch::channel(None);
                            calls.insert(frame.call_id, Pending { sequence: 1, response_window: response_window.clone(), sender: Some(sender), cancel: cancel.clone(), failure });
                            let out = writer.call(frame.call_id)?;
                            let dispatcher = dispatcher.clone();
                            let id = frame.call_id;
                            let timeout = frame.timeout_ms;
                            tasks.spawn(async move {
                                let result = dispatch(dispatcher, receiver, out, Invocation { id, timeout_ms: timeout, cancel, failed, window: response_window }).await;
                                (id, result)
                            });
                        }
                        let pending = calls.get_mut(&frame.call_id).expect("live call");
                        if pending.failure.borrow().is_some() { continue; }
                        if frame.sequence != pending.sequence || pending.sender.is_none() { return Err(invalid_frame()); }
                        pending.sequence += 1;
                        if let Some(request) = frame.request {
                            dispatcher.authorize(&node_pb::decode_auth(agent_protocol::connection_pb::auth(&request).cloned())?)?;
                            if pending.sender.as_ref().expect("open requests").try_send(Ok(request)).is_err() {
                                pending.failure.send_replace(Some(overloaded()));
                                pending.sender.take();
                                continue;
                            }
                        } else if !frame.requests_closed { return Err(invalid_frame()); }
                        if frame.requests_closed { pending.sender.take(); }
                    }
                    _ => return Err(invalid_frame()),
                }
            }
        }.await;
    stop.cancel();
    calls.clear();
    while let Some(joined) = tasks.join_next().await {
        let (_, drained) = joined.map_err(|error| {
            TalosError::internal_with_source(
                "Drain Agent invocation task; technical details were withheld.",
                error,
            )
        })?;
        // Closing a peer may already have closed its ordered result queue.
        if let Err(error) = drained {
            if !error.is_code(ErrorCode::ServiceUnavailable) {
                return Err(error);
            }
        }
    }
    result
}

async fn dispatch(
    dispatcher: Arc<Dispatcher>,
    requests: mpsc::Receiver<Result<Call, tonic::Status>>,
    out: mpsc::Sender<pb::NodeToControlFrame>,
    invocation: Invocation,
) -> TalosResult<()> {
    let Invocation {
        id,
        timeout_ms,
        cancel,
        mut failed,
        window,
    } = invocation;
    let mut sequence = 1;
    let deadline = async {
        if timeout_ms == 0 {
            futures_util::future::pending::<()>().await;
        } else {
            tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
        }
    };
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Ok(()),
        _ = deadline => Err(TalosError::new(ErrorCode::Timeout, "Agent invocation timed out")),
        _ = failed.changed() => Err(failed.borrow().clone().unwrap_or_else(closed)),
        result = async {
            let mut replies = dispatcher.dispatch(super::transport_stream::RpcStream::new(ReceiverStream::new(requests))).await.map_err(crate::decode_rpc_status)?;
            while let Some(reply) = replies.message().await.map_err(crate::decode_rpc_status)? {
                window.reserve().await?;
                // Reserve one terminal slot, so an overload failure never overtakes prior frames.
                let permits = tokio::time::timeout(Duration::from_secs(5), out.reserve_many(2)).await
                    .map_err(|_| overloaded())?.map_err(|_| closed())?;
                let frame = result_frame(id, sequence, false, Some(reply));
                window.sent(sequence);
                permits.into_iter().next().expect("reserved slot").send(frame);
                sequence += 1;
            }
            Ok(())
        } => result,
    };
    let response = result.err().map(|error| {
        pb::worker_rpc_result::Response::Failure(Box::new(error_pb::encode_failure(&error)))
    });
    out.try_send(result_frame(id, sequence, true, response))
        .map_err(|_| closed())
}

fn result_frame(
    call_id: u64,
    sequence: u64,
    completed: bool,
    response: Option<pb::worker_rpc_result::Response>,
) -> pb::NodeToControlFrame {
    pb::NodeToControlFrame {
        body: Some(pb::node_to_control_frame::Body::Result(
            pb::WorkerRpcResult {
                call_id,
                sequence,
                completed,
                response,
            },
        )),
    }
}
fn send_failure(
    writer: &Writer<pb::NodeToControlFrame>,
    id: u64,
    sequence: u64,
    error: TalosError,
) -> TalosResult<()> {
    writer.control(result_frame(
        id,
        sequence,
        true,
        Some(pb::worker_rpc_result::Response::Failure(Box::new(
            error_pb::encode_failure(&error),
        ))),
    ))
}
fn invalid_frame() -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, "Agent Connect frame is invalid")
}
