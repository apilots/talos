//! Accept outbound execution streams into the existing node publication owner.
use super::*;
use agent_client::AgentExecutionConnection;
use std::pin::Pin;
use talos_service::AgentNodeRevocationHook;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Streaming;

type ConnectStream =
    Pin<Box<dyn tokio_stream::Stream<Item = Result<pb::ControlToNodeFrame, Status>> + Send>>;
impl NodeControlRpc {
    pub(super) async fn accept_connection(
        &self,
        mut input: Streaming<pb::NodeToControlFrame>,
    ) -> Result<Response<ConnectStream>, Status> {
        let first = tokio::time::timeout(std::time::Duration::from_secs(10), input.message())
            .await
            .map_err(|error| {
                control_status(TalosError::from_source(
                    ErrorCode::Timeout,
                    "Agent Connect handshake timed out",
                    error,
                ))
            })?
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
        let lifecycle = self.nodes.lock_lifecycle().await;
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
                connection.close();
                self.remote_publisher.unpublish(&auth.node_id).await;
                return Err(control_status(error));
            }
        }
        let environment = Arc::new(agent_client::AgentEnvironmentClient::from_connection(
            connection.clone(),
            auth.clone(),
        ));
        self.remote_publisher
            .coordinator
            .register_environment(auth.node_id.clone(), environment.clone())
            .await;
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
        if let Err(error) = connection.ready() {
            self.nodes.withdraw_publication(&auth, &revoke).await;
            return Err(control_status(error));
        }
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
            let _lifecycle = nodes.lock_lifecycle().await;
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
