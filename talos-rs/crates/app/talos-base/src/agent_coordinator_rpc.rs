//! Gateway-only relay. Business admission and persistence stay at the caller.
use super::*;
use std::pin::Pin;
use tokio_stream::Stream;
use tonic::Streaming;

pub(super) struct CoordinatorRpc {
    management: NodeManagementRpc,
    publisher: RemoteNodePublisher,
    shutdown: CancellationToken,
}
impl CoordinatorRpc {
    pub(super) fn new(
        management: NodeManagementRpc,
        publisher: RemoteNodePublisher,
        shutdown: CancellationToken,
    ) -> Self {
        Self {
            management,
            publisher,
            shutdown,
        }
    }
}
#[tonic::async_trait]
impl pb::agent_coordinator_rpc_server::AgentCoordinatorRpc for CoordinatorRpc {
    async fn invoke_active_command(
        &self,
        request: Request<pb::ActiveSessionCommandRequest>,
    ) -> Result<Response<pb::ActiveSessionCommandResponse>, Status> {
        use futures_util::StreamExt;
        let actor = self.management.actor(&request).await.map_err(status)?;
        let (target, _) =
            agent_protocol::coordinator_commands_pb::decode_request(request.get_ref().clone())
                .map_err(status)?;
        let node = self
            .management
            .nodes
            .get_node(
                &actor,
                agent_protocol::GetNodeRequest {
                    node_id: target.computer_id,
                },
            )
            .await
            .map_err(status)?;
        if node.owner_user_id.as_deref() != Some(&actor.user_id) {
            return Err(status(TalosError::new(
                ErrorCode::PermissionDenied,
                "Computer belongs to another account.",
            )));
        }
        let mut responses = self
            .publisher
            .resource_routes
            .forward_peer(
                &actor.user_id,
                &target.coordinator_id,
                pb::worker_rpc_call::Request::ActiveSessionCommand(Box::new(request.into_inner())),
            )
            .await?;
        let response = responses
            .next()
            .await
            .transpose()?
            .ok_or_else(|| status(invalid()))?;
        let pb::worker_rpc_result::Response::ActiveSessionCommand(response) = response else {
            return Err(status(invalid()));
        };
        Ok(Response::new(*response))
    }

    type ConnectStream =
        Pin<Box<dyn Stream<Item = Result<pb::ControlToCoordinatorFrame, Status>> + Send>>;
    async fn connect(
        &self,
        request: Request<Streaming<pb::CoordinatorToControlFrame>>,
    ) -> Result<Response<Self::ConnectStream>, Status> {
        let actor = self.management.actor(&request).await.map_err(status)?;
        let mut input = request.into_inner();
        let first = tokio::time::timeout(std::time::Duration::from_secs(10), input.message())
            .await
            .map_err(|error| {
                status(TalosError::from_source(
                    ErrorCode::Timeout,
                    "Coordinator handshake timed out",
                    error,
                ))
            })?
            .map_err(|error| status(error_pb::decode_status(error)))?
            .ok_or_else(|| status(invalid()))?;
        let Some(pb::coordinator_to_control_frame::Body::Connect(first)) = first.body else {
            return Err(status(invalid()));
        };
        if uuid::Uuid::parse_str(&first.peer_id).is_err() {
            return Err(status(invalid()));
        }
        let node = self
            .management
            .nodes
            .get_node(
                &actor,
                agent_protocol::GetNodeRequest {
                    node_id: first.computer_id.clone(),
                },
            )
            .await
            .map_err(status)?;
        if node.owner_user_id.as_deref() != Some(&actor.user_id) {
            return Err(status(TalosError::new(
                ErrorCode::PermissionDenied,
                "Computer belongs to another account",
            )));
        }
        if !node.available {
            return Err(status(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Computer is unavailable",
            )));
        }
        let target = self
            .publisher
            .connections
            .lock()
            .await
            .get(&first.computer_id)
            .filter(|target| !target.is_closed())
            .cloned()
            .ok_or_else(|| {
                status(TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "Computer Connect is unavailable",
                ))
            })?;
        let stop = self.shutdown.child_token();
        let auth = NodeAuth {
            node_id: first.computer_id,
            token: actor
                .access_token
                .clone()
                .ok_or_else(|| status(invalid()))?,
        };
        let connection_id = uuid::Uuid::new_v4().to_string();
        let (output, mut task, resource_connection) = agent_client::serve_coordinator_connection(
            target.clone(),
            auth.clone(),
            connection_id.clone(),
            first.peer_id.clone(),
            input,
            stop.clone(),
        )
        .await;
        self.publisher
            .resource_routes
            .install(
                connection_id.clone(),
                first.peer_id.clone(),
                first.serves_files,
                actor.user_id.clone(),
                auth.node_id.clone(),
                &resource_connection,
            )
            .await
            .map_err(|error| {
                stop.cancel();
                status(error)
            })?;
        let peer_id = connection_id;
        let resource_routes = self.publisher.resource_routes.clone();
        let management = self.management.clone();
        tokio::spawn(async move {
            let mut check = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = stop.cancelled() => break,
                    _ = target.ended() => break,
                    result = &mut task => {
                        match result {
                            Ok(Err(error)) if !matches!(error.error_code(), ErrorCode::ServiceUnavailable | ErrorCode::Cancelled) => talos_logging::report_error(&error),
                            Err(error) => talos_logging::report_error(&TalosError::internal_with_source("Coordinator relay task failed; technical details were withheld.", error)),
                            _ => {}
                        }
                        resource_connection.close();
                        resource_routes.remove(&peer_id, &resource_connection).await;
                        return;
                    }
                    _ = check.tick() => {
                        let verification = tokio::select! {
                            _ = stop.cancelled() => break,
                            _ = target.ended() => break,
                            result = tokio::time::timeout(std::time::Duration::from_secs(10), management.auth_service.verify_access_token(actor.access_token.as_deref().expect("verified actor"))) => result,
                        };
                        if !matches!(verification, Ok(Ok(identity)) if !identity.auth_disabled && identity.user_id == actor.user_id) { break; }
                    }
                }
            }
            stop.cancel();
            resource_connection.close();
            resource_routes.remove(&peer_id, &resource_connection).await;
            if let Err(error) = task.await {
                talos_logging::report_error(&TalosError::internal_with_source(
                    "Coordinator relay task failed; technical details were withheld.",
                    error,
                ));
            }
        });
        Ok(Response::new(output))
    }
}
fn invalid() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "Coordinator handshake is invalid",
    )
}
fn status(error: TalosError) -> Status {
    talos_logging::report_error(&error);
    error_pb::encode_status(&error)
}
