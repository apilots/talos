//! Accept outbound execution streams into the existing node publication owner.
use super::*;
use agent_client::AgentExecutionConnection;
use axum_ws::extract::ws::{WebSocket, WebSocketUpgrade};
use futures_util::{Stream, StreamExt};
use prost::Message;
use std::pin::Pin;
use talos_service::AgentNodeRevocationHook;
use tokio_stream::wrappers::ReceiverStream;

const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

type ConnectStream =
    Pin<Box<dyn tokio_stream::Stream<Item = Result<pb::ControlToNodeFrame, Status>> + Send>>;
impl NodeControlRpc {
    /// WSS is only a carrier for the existing node lifecycle and typed Connect.
    /// Actor management/resource APIs and cloud gRPC retain their original routes.
    pub(super) fn websocket_routes(self) -> axum_ws::Router {
        use axum_ws::routing::get;
        let mut router = axum_ws::Router::new();
        macro_rules! unary {
            ($path:literal, $request:ty, $method:ident) => {{
                let control = self.clone();
                router = router.route(
                    $path,
                    get(move |upgrade: WebSocketUpgrade| {
                        let control = control.clone();
                        async move {
                            upgrade
                                .max_message_size(MAX_FRAME_BYTES)
                                .max_frame_size(MAX_FRAME_BYTES)
                                .on_upgrade(move |socket| async move {
                                    let result = websocket_unary::<$request, _, _, _>(
                                        socket,
                                        move |request| async move {
                                            use pb::agent_node_control_server::AgentNodeControl;
                                            control
                                                .$method(Request::new(request))
                                                .await
                                                .map(Response::into_inner)
                                        },
                                    )
                                    .await;
                                    if let Err(error) = result {
                                        talos_logging::report_error(&error);
                                    }
                                })
                        }
                    }),
                );
            }};
        }
        unary!("/talos/worker/register", pb::RegisterRequest, register);
        unary!(
            "/talos/worker/renew_lease",
            pb::RenewLeaseRequest,
            renew_lease
        );
        unary!("/talos/worker/heartbeat", pb::HeartbeatRequest, heartbeat);
        unary!(
            "/talos/worker/unregister",
            pb::UnregisterRequest,
            unregister
        );
        router.route(
            "/talos/worker/connect",
            get(move |upgrade: WebSocketUpgrade| {
                let control = self.clone();
                async move {
                    upgrade
                        .max_message_size(MAX_FRAME_BYTES)
                        .max_frame_size(MAX_FRAME_BYTES)
                        .on_upgrade(move |socket| async move {
                            if let Err(error) = control.serve_websocket(socket).await {
                                talos_logging::report_error(&error);
                            }
                        })
                }
            }),
        )
    }

    async fn serve_websocket(&self, socket: WebSocket) -> TalosResult<()> {
        use axum_ws::extract::ws::Message as WsMessage;
        use futures_util::SinkExt;
        let (mut sink, input) = socket.split();
        // Dropping the writer must also end the accepted reader/publication,
        // including on size rejection or a stalled/disconnected socket.
        let closed = CancellationToken::new();
        let _close = closed.clone().drop_guard();
        let input = input
            .take_until(closed.cancelled_owned())
            .filter_map(|message| async move {
                match message {
                    Ok(WsMessage::Binary(bytes)) => {
                        Some(pb::NodeToControlFrame::decode(bytes).map_err(|error| {
                            error_pb::encode_status(&TalosError::from_source(
                                ErrorCode::InvalidRequest,
                                "Agent WebSocket frame is malformed",
                                error,
                            ))
                        }))
                    }
                    Ok(WsMessage::Ping(_) | WsMessage::Pong(_)) => None,
                    Ok(_) => Some(Err(error_pb::encode_status(&TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Agent WebSocket closed",
                    )))),
                    Err(error) => Some(Err(error_pb::encode_status(&TalosError::from_source(
                        ErrorCode::ServiceUnavailable,
                        "Agent WebSocket receive failed",
                        error,
                    )))),
                }
            });
        let mut output = match self.accept_connection(input).await {
            Ok(response) => response.into_inner(),
            Err(status) => {
                let error = error_pb::decode_status(status);
                let failure = pb::ControlToNodeFrame {
                    body: Some(pb::control_to_node_frame::Body::Connected(
                        pb::ConnectNodeResponse {
                            result: Some(pb::connect_node_response::Result::Failure(
                                error_pb::encode_failure(&error),
                            )),
                            stream_heartbeat: false,
                        },
                    )),
                };
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    sink.send(WsMessage::Binary(failure.encode_to_vec().into())),
                )
                .await;
                // The shared ingress already owns reporting this authorization error.
                return Ok(());
            }
        };
        loop {
            let frame = tokio::select! {
                _ = self.shutdown.cancelled() => return Ok(()),
                frame = output.next() => match frame { Some(frame) => frame.map_err(error_pb::decode_status)?, None => return Ok(()) },
            };
            if frame.encoded_len() > MAX_FRAME_BYTES {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Agent WebSocket frame exceeds the transport limit",
                ));
            }
            tokio::select! {
                _ = self.shutdown.cancelled() => return Ok(()),
                result = tokio::time::timeout(std::time::Duration::from_secs(5), sink.send(WsMessage::Binary(frame.encode_to_vec().into()))) => {
                    result.map_err(|error| TalosError::from_source(ErrorCode::Timeout, "Agent WebSocket send timed out", error))?
                        .map_err(|error| TalosError::from_source(ErrorCode::ServiceUnavailable, "Agent WebSocket send failed", error))?;
                },
            }
        }
    }

    pub(super) async fn accept_connection<S>(
        &self,
        input: S,
    ) -> Result<Response<ConnectStream>, Status>
    where
        S: Stream<Item = Result<pb::NodeToControlFrame, Status>> + Send + 'static,
    {
        let mut input = Box::pin(input);
        let first = tokio::time::timeout(std::time::Duration::from_secs(10), input.next())
            .await
            .map_err(|error| {
                control_status(TalosError::from_source(
                    ErrorCode::Timeout,
                    "Agent Connect handshake timed out",
                    error,
                ))
            })?
            .transpose()
            .map_err(|status| control_status(error_pb::decode_status(status)))?
            .ok_or_else(|| {
                control_status(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Agent Connect handshake is missing",
                ))
            })?;
        let Some(pb::node_to_control_frame::Body::Connect(first)) = first.body else {
            return Err(control_status(TalosError::new(
                ErrorCode::InvalidRequest,
                "Agent Connect handshake is invalid",
            )));
        };
        let auth = node_pb::decode_auth(first.auth).map_err(control_status)?;
        node_pb::require_empty(first.params).map_err(control_status)?;
        let lifecycle = self.nodes.lock_lifecycle(&auth.node_id).await;
        let deployments = self
            .nodes
            .outbound_deployments(&auth)
            .await
            .map_err(control_status)?;
        self.nodes
            .revoke_publication(&auth)
            .await
            .map_err(control_status)?;
        let (connection, output) = AgentExecutionConnection::accept(auth.clone(), input);
        let environment = Arc::new(agent_client::AgentEnvironmentClient::from_connection(
            connection.clone(),
            auth.clone(),
        ));
        let admission = environment.clone();
        self.nodes
            .set_registration_deletion_hook(
                &auth,
                Arc::new(move |blocked| {
                    let admission = admission.clone();
                    Box::pin(async move { admission.set_registration_admission(blocked).await })
                }),
            )
            .await
            .map_err(control_status)?;
        let coordinator = self.remote_publisher.coordinator.clone();
        let revoke_node = auth.node_id.clone();
        let revoke_connection = connection.clone();
        let revoke: AgentNodeRevocationHook = Arc::new(move || {
            let coordinator = coordinator.clone();
            let node_id = revoke_node.clone();
            let connection = revoke_connection.clone();
            Box::pin(async move {
                connection.close();
                coordinator.unregister_remote_node(&node_id).await;
            })
        });
        if let Err(error) = self
            .nodes
            .mark_registration_published(&auth, revoke.clone())
            .await
        {
            connection.close();
            self.remote_publisher.unpublish(&auth.node_id).await;
            return Err(control_status(error));
        }
        let ready = if first.stream_heartbeat {
            connection.ready_with_heartbeat()
        } else {
            connection.ready()
        };
        if let Err(error) = ready {
            self.nodes.withdraw_publication(&auth, &revoke).await;
            return Err(control_status(error));
        }
        // Queue Connected before publishing a callable client. A caller may
        // dispatch immediately when the coordinator exposes the environment;
        // otherwise its Call can become the Worker's first handshake frame.
        for deployment in deployments {
            let client = RemoteAgentClient::from_connection(
                connection.clone(),
                deployment.agent_id.clone(),
                auth.clone(),
            )
            .with_coordinator_id(self.remote_publisher.coordinator_id.clone())
            .map_err(control_status)?;
            if let Err(error) = self
                .remote_publisher
                .coordinator
                .replace_remote_agent(auth.node_id.clone(), deployment.agent_id, Arc::new(client))
                .await
            {
                self.nodes.withdraw_publication(&auth, &revoke).await;
                return Err(control_status(error));
            }
        }
        self.remote_publisher
            .coordinator
            .register_environment(auth.node_id.clone(), environment.clone())
            .await;
        self.remote_publisher
            .connections
            .lock()
            .await
            .insert(auth.node_id.clone(), environment);
        drop(lifecycle);
        let nodes = self.nodes.clone();
        let shutdown = self.shutdown.clone();
        tokio::spawn(async move {
            let mut check = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => { connection.close(); break; }
                    _ = connection.ended() => break,
                    _ = check.tick() => {
                        if nodes.authenticate_lease(&auth).await.is_err() { connection.close(); break; }
                    }
                }
            }
            let _lifecycle = nodes.lock_lifecycle(&auth.node_id).await;
            nodes.withdraw_publication(&auth, &revoke).await;
        });
        Ok(Response::new(Box::pin(tokio_stream::StreamExt::map(
            ReceiverStream::new(output),
            Ok,
        ))))
    }
}

fn control_status(error: TalosError) -> Status {
    talos_logging::report_error(&error);
    error_pb::encode_status(&error)
}

async fn websocket_unary<T, R, F, Fut>(mut socket: WebSocket, call: F) -> TalosResult<()>
where
    T: Message + Default,
    R: Message,
    F: FnOnce(T) -> Fut,
    Fut: std::future::Future<Output = Result<R, Status>>,
{
    use axum_ws::extract::ws::Message as WsMessage;
    let message = tokio::time::timeout(std::time::Duration::from_secs(10), socket.recv())
        .await
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::Timeout,
                "Agent WebSocket request timed out",
                error,
            )
        })?
        .ok_or_else(|| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "Agent WebSocket request is missing",
            )
        })?
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::ServiceUnavailable,
                "Agent WebSocket receive failed",
                error,
            )
        })?;
    let WsMessage::Binary(bytes) = message else {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Agent WebSocket request must be binary",
        ));
    };
    let request = T::decode(bytes).map_err(|error| {
        TalosError::from_source(
            ErrorCode::InvalidRequest,
            "Agent WebSocket request is malformed",
            error,
        )
    })?;
    let response = call(request).await.map_err(error_pb::decode_status)?;
    socket
        .send(WsMessage::Binary(response.encode_to_vec().into()))
        .await
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::ServiceUnavailable,
                "Agent WebSocket response failed",
                error,
            )
        })
}
