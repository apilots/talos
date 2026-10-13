//! Account-authenticated callers reuse Node Connect framing in both directions.
use crate::{
    connection_writer::{closed, Writer, CALL_CAPACITY},
    AgentExecutionConnection, AgentNodeClient, AgentResourceRpcHandler,
};
use agent_pb as pb;
use agent_protocol::{error_pb, NodeAuth};
use futures_util::{Stream, StreamExt};
use std::{pin::Pin, sync::Arc, time::Duration};
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tonic::{Request, Status};

impl AgentNodeClient {
    /// Bind an outgoing stream to one owned Computer. Several streams multiplex
    /// over one HTTP/2 channel; callers never receive the target Worker's lease.
    pub async fn connect_coordinator(
        &self,
        token: &str,
        peer_id: &str,
        computer_id: &str,
    ) -> TalosResult<(Arc<AgentExecutionConnection>, NodeAuth)> {
        self.open_coordinator(token, peer_id, computer_id, None)
            .await
    }
    /// Supply original management handlers for invocation-scoped callbacks.
    /// The SDK verifies the live invocation; handlers retain business authorization.
    pub async fn connect_coordinator_with_resources(
        &self,
        token: &str,
        peer_id: &str,
        computer_id: &str,
        resources: Arc<dyn AgentResourceRpcHandler>,
    ) -> TalosResult<(Arc<AgentExecutionConnection>, NodeAuth)> {
        self.open_coordinator(token, peer_id, computer_id, Some(resources))
            .await
    }
    async fn open_coordinator(
        &self,
        token: &str,
        peer_id: &str,
        computer_id: &str,
        resources: Option<Arc<dyn AgentResourceRpcHandler>>,
    ) -> TalosResult<(Arc<AgentExecutionConnection>, NodeAuth)> {
        if token.is_empty() || peer_id.is_empty() || computer_id.is_empty() {
            return Err(invalid());
        }
        let auth = NodeAuth {
            node_id: computer_id.into(),
            token: token.into(),
        };
        let (sender, receiver) = mpsc::channel(CALL_CAPACITY);
        sender
            .send(pb::CoordinatorToControlFrame {
                body: Some(pb::coordinator_to_control_frame::Body::Connect(
                    pb::ConnectCoordinatorRequest {
                        peer_id: peer_id.into(),
                        computer_id: computer_id.into(),
                        serves_files: resources
                            .as_ref()
                            .is_some_and(|handler| handler.serves_files()),
                    },
                )),
            })
            .await
            .map_err(|_| closed())?;
        let mut request = Request::new(ReceiverStream::new(receiver));
        request.metadata_mut().insert(
            "authorization",
            format!("Bearer {token}").parse().map_err(|_| invalid())?,
        );
        let mut input = tokio::time::timeout(
            Duration::from_secs(10),
            pb::agent_coordinator_rpc_client::AgentCoordinatorRpcClient::new(self.channel.clone())
                .connect(request),
        )
        .await
        .map_err(|error| {
            TalosError::from_source(ErrorCode::Timeout, "Coordinator Connect timed out", error)
        })?
        .map_err(crate::decode_rpc_status)?
        .into_inner();
        let first = tokio::time::timeout(Duration::from_secs(10), input.message())
            .await
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::Timeout,
                    "Coordinator acknowledgement timed out",
                    error,
                )
            })?
            .map_err(crate::decode_rpc_status)?
            .ok_or_else(closed)?;
        if !matches!(
            first.body,
            Some(pb::control_to_coordinator_frame::Body::Connected(
                pb::ConnectNodeResponse {
                    result: Some(pb::connect_node_response::Result::Ready(_))
                }
            ))
        ) {
            return Err(invalid());
        }
        let stop = CancellationToken::new();
        let (callback_sender, callback_input) = mpsc::channel(CALL_CAPACITY);
        let input = input.filter_map(move |frame| {
            let sender = callback_sender.clone();
            async move {
                let frame = match frame {
                    Ok(frame) => frame,
                    Err(error) => return Some(Err(error)),
                };
                use pb::control_to_coordinator_frame::Body;
                let body = match frame.body {
                    Some(Body::Result(result)) => {
                        return Some(Ok(pb::NodeToControlFrame {
                            body: Some(pb::node_to_control_frame::Body::Result(result)),
                        }))
                    }
                    Some(Body::ResourceCall(value)) => pb::control_to_node_frame::Body::Call(value),
                    Some(Body::ResourceCancel(value)) => {
                        pb::control_to_node_frame::Body::Cancel(value)
                    }
                    Some(Body::ResourceAcknowledge(value)) => {
                        pb::control_to_node_frame::Body::Acknowledge(value)
                    }
                    _ => return Some(Err(error_pb::encode_status(&invalid()))),
                };
                if tokio::time::timeout(
                    Duration::from_secs(5),
                    sender.send(Ok(pb::ControlToNodeFrame { body: Some(body) })),
                )
                .await
                .map_or(true, |result| result.is_err())
                {
                    return Some(Err(error_pb::encode_status(&closed())));
                }
                None
            }
        });
        let (connection, mut calls) = AgentExecutionConnection::accept(auth.clone(), input);
        let (writer, mut replies) = Writer::new(stop.clone());
        let callback_task = tokio::spawn(crate::worker_connection::serve_calls(
            Box::pin(ReceiverStream::new(callback_input)),
            writer,
            stop.clone(),
            crate::worker_connection::Dispatcher::Resources {
                handler: resources.unwrap_or_else(|| Arc::new(NoResources)),
                auth: auth.clone(),
                invocation: Arc::downgrade(&connection),
            },
        ));
        let task_connection = connection.clone();
        tokio::spawn(async move {
            loop {
                let body = tokio::select! {
                    _ = task_connection.ended() => break,
                    frame = calls.recv() => match frame.and_then(|frame| frame.body) {
                        Some(pb::control_to_node_frame::Body::Call(value)) => pb::coordinator_to_control_frame::Body::Call(value),
                        Some(pb::control_to_node_frame::Body::Cancel(value)) => pb::coordinator_to_control_frame::Body::Cancel(value),
                        Some(pb::control_to_node_frame::Body::Acknowledge(value)) => pb::coordinator_to_control_frame::Body::Acknowledge(value),
                        _ => break,
                    },
                    frame = replies.recv() => match frame.and_then(|frame| frame.body) {
                        Some(pb::node_to_control_frame::Body::Result(value)) => pb::coordinator_to_control_frame::Body::ResourceResult(value),
                        _ => break,
                    },
                };
                if tokio::time::timeout(
                    Duration::from_secs(5),
                    sender.send(pb::CoordinatorToControlFrame { body: Some(body) }),
                )
                .await
                .map_or(true, |result| result.is_err())
                {
                    break;
                }
            }
            task_connection.close();
            stop.cancel();
            match callback_task.await {
                Ok(Err(error))
                    if !matches!(
                        error.error_code(),
                        ErrorCode::ServiceUnavailable | ErrorCode::Cancelled
                    ) =>
                {
                    talos_logging::report_error(&error)
                }
                Err(error) => talos_logging::report_error(&TalosError::internal_with_source(
                    "Coordinator resource task failed; technical details were withheld.",
                    error,
                )),
                _ => {}
            }
        });
        Ok((connection, auth))
    }
}

/// Serve a verified management stream without running a product Coordinator.
/// The owner validates account, Computer ownership, token lifetime and routes
/// resource calls through the returned live connection. Nothing is persisted.
pub async fn serve_coordinator_connection<S>(
    target: Arc<crate::AgentEnvironmentClient>,
    auth: NodeAuth,
    peer_id: String,
    coordinator_id: String,
    input: S,
    stop: CancellationToken,
) -> (
    Pin<Box<dyn Stream<Item = Result<pb::ControlToCoordinatorFrame, Status>> + Send>>,
    tokio::task::JoinHandle<TalosResult<()>>,
    Arc<AgentExecutionConnection>,
)
where
    S: Stream<Item = Result<pb::CoordinatorToControlFrame, Status>> + Send + 'static,
{
    let (result_sender, results) = mpsc::channel(CALL_CAPACITY);
    let input = input.filter_map(move |frame| {
        let sender = result_sender.clone();
        async move {
            let frame = match frame {
                Ok(frame) => frame,
                Err(error) => return Some(Err(error)),
            };
            use pb::coordinator_to_control_frame::Body;
            let body = match frame.body {
                Some(Body::Call(value)) => pb::control_to_node_frame::Body::Call(value),
                Some(Body::Cancel(value)) => pb::control_to_node_frame::Body::Cancel(value),
                Some(Body::Acknowledge(value)) => {
                    pb::control_to_node_frame::Body::Acknowledge(value)
                }
                Some(Body::ResourceResult(value)) => {
                    if tokio::time::timeout(
                        Duration::from_secs(5),
                        sender.send(Ok(pb::NodeToControlFrame {
                            body: Some(pb::node_to_control_frame::Body::Result(value)),
                        })),
                    )
                    .await
                    .map_or(true, |result| result.is_err())
                    {
                        return Some(Err(error_pb::encode_status(&closed())));
                    }
                    return None;
                }
                _ => return Some(Err(error_pb::encode_status(&invalid()))),
            };
            Some(Ok(pb::ControlToNodeFrame { body: Some(body) }))
        }
    });
    let (resources, mut callbacks) =
        AgentExecutionConnection::accept(auth.clone(), ReceiverStream::new(results));
    let (writer, mut results) = Writer::new(stop.clone());
    let worker_task = tokio::spawn(crate::worker_connection::serve_calls(
        Box::pin(input),
        writer,
        stop.clone(),
        crate::worker_connection::Dispatcher::Relay {
            connection: target,
            auth,
            peer_id,
            coordinator_id,
        },
    ));
    let (sender, output) = mpsc::channel(CALL_CAPACITY);
    let resource_lifetime = resources.clone();
    let relay_stop = stop.clone();
    let task = tokio::spawn(async move {
        loop {
            let body = tokio::select! {
                _ = relay_stop.cancelled() => break,
                _ = sender.closed() => break,
                frame = results.recv() => match frame.and_then(|frame| frame.body) {
                    Some(pb::node_to_control_frame::Body::Result(value)) => pb::control_to_coordinator_frame::Body::Result(value),
                    _ => break,
                },
                frame = callbacks.recv() => match frame.and_then(|frame| frame.body) {
                    Some(pb::control_to_node_frame::Body::Call(value)) => pb::control_to_coordinator_frame::Body::ResourceCall(value),
                    Some(pb::control_to_node_frame::Body::Cancel(value)) => pb::control_to_coordinator_frame::Body::ResourceCancel(value),
                    Some(pb::control_to_node_frame::Body::Acknowledge(value)) => pb::control_to_coordinator_frame::Body::ResourceAcknowledge(value),
                    _ => break,
                },
            };
            if tokio::time::timeout(
                Duration::from_secs(5),
                sender.send(Ok(pb::ControlToCoordinatorFrame { body: Some(body) })),
            )
            .await
            .map_or(true, |result| result.is_err())
            {
                break;
            }
        }
        relay_stop.cancel();
        resource_lifetime.close();
        worker_task.await.map_err(|error| {
            TalosError::internal_with_source(
                "Coordinator carrier task failed; technical details were withheld.",
                error,
            )
        })?
    });
    let ready = tokio_stream::once(Ok(pb::ControlToCoordinatorFrame {
        body: Some(pb::control_to_coordinator_frame::Body::Connected(
            pb::ConnectNodeResponse {
                result: Some(pb::connect_node_response::Result::Ready(pb::Empty {})),
            },
        )),
    }));
    (
        Box::pin(ready.chain(ReceiverStream::new(output))),
        task,
        resources,
    )
}
struct NoResources;
impl AgentResourceRpcHandler for NoResources {
    fn dispatch(
        &self,
        _: pb::CoordinatorCallContext,
        _: crate::AgentResourceRequests,
    ) -> futures_util::future::BoxFuture<'_, Result<crate::AgentResourceResponses, Status>> {
        Box::pin(async {
            Err(error_pb::encode_status(&TalosError::new(
                ErrorCode::PermissionDenied,
                "Caller did not provide resource handlers",
            )))
        })
    }
}
fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Coordinator Connect frame is invalid",
    )
}
