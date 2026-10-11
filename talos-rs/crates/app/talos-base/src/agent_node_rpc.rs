//! Control-plane gRPC ingress for node lifecycle and owner management.

use std::sync::Arc;
use std::{fs, net::SocketAddr};

use agent_client::{AgentTlsTrust, RemoteAgentClient};
use agent_pb as pb;
use agent_protocol::{error_pb, node_pb, AgentHarness, Deployment, NodeAuth, Registration};
use talos_auth::AuthService;
use talos_config::AgentNodeControlConfig;
use talos_coordinator::CoordinatorImpl;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::{Actor, AgentNodeService};
use talos_tools::AgentControl;
use tokio_util::sync::CancellationToken;
use tonic::transport::{Identity, Server, ServerTlsConfig};
use tonic::{Request, Response, Status};

#[path = "agent_node_resources_rpc.rs"]
mod resources;
pub use resources::{NodeSessionDataRpc, NodeSubagentControlRpc, NodeToolImageRpc};
#[path = "agent_node_connect.rs"]
mod connect;

pub struct NodeRpcServer {
    bind_address: SocketAddr,
    identity: Identity,
    nodes: AgentNodeService,
    auth_service: Arc<dyn AuthService>,
    session_data: NodeSessionDataRpc,
    subagent_control: NodeSubagentControlRpc,
    tool_images: NodeToolImageRpc,
    remote_publisher: RemoteNodePublisher,
}

impl NodeRpcServer {
    /// Compose the complete node listener with its existing resource owners.
    /// Direct registration publishes execution clients; outbound registration awaits Connect.
    /// Callers must supply all resource services before starting the listener.
    pub fn new(
        config: &AgentNodeControlConfig,
        nodes: AgentNodeService,
        auth_service: Arc<dyn AuthService>,
        coordinator: Arc<CoordinatorImpl>,
        session_data: NodeSessionDataRpc,
        agent_control: Arc<dyn AgentControl>,
        tool_images: NodeToolImageRpc,
    ) -> TalosResult<Self> {
        let bind_address = config.bind_address.parse().map_err(|error| {
            TalosError::from_source(
                ErrorCode::InvalidRequest,
                "Agent node control bind address is invalid",
                error,
            )
        })?;
        let certificate = fs::read(&config.tls_certificate_file).map_err(|error| {
            TalosError::internal_with_source(
                "Read Agent node control TLS certificate; technical details were withheld.",
                error,
            )
        })?;
        let private_key = fs::read(&config.tls_private_key_file).map_err(|error| {
            TalosError::internal_with_source(
                "Read Agent node control TLS key; technical details were withheld.",
                error,
            )
        })?;
        let trusted_node_ca = config
            .trusted_node_ca_file
            .as_ref()
            .map(fs::read)
            .transpose()
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Read trusted Agent node CA; technical details were withheld.",
                    error,
                )
            })?;
        let remote_publisher = RemoteNodePublisher {
            coordinator,
            nodes: nodes.clone(),
            trusted_node_ca: trusted_node_ca
                .map_or(AgentTlsTrust::SystemRoots, AgentTlsTrust::Certificate),
        };
        let subagent_control = NodeSubagentControlRpc::new(session_data.clone(), agent_control);
        Ok(Self {
            bind_address,
            identity: Identity::from_pem(certificate, private_key),
            nodes,
            auth_service,
            session_data,
            subagent_control,
            tool_images,
            remote_publisher,
        })
    }

    pub async fn run(self, shutdown: CancellationToken) -> TalosResult<()> {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let control =
            pb::agent_node_control_server::AgentNodeControlServer::new(NodeControlRpc::new(
                self.nodes.clone(),
                self.remote_publisher.clone(),
                shutdown.clone(),
            ));
        let management = pb::agent_node_management_server::AgentNodeManagementServer::new(
            NodeManagementRpc::new(self.nodes, self.auth_service),
        );
        let session_data =
            pb::agent_session_data_rpc_server::AgentSessionDataRpcServer::new(self.session_data);
        let subagent_control =
            pb::agent_subagent_control_rpc_server::AgentSubagentControlRpcServer::new(
                self.subagent_control,
            );
        let tool_images =
            pb::agent_tool_image_rpc_server::AgentToolImageRpcServer::new(self.tool_images);
        Server::builder()
            .tls_config(ServerTlsConfig::new().identity(self.identity))
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Configure Agent node control TLS; technical details were withheld.",
                    error,
                )
            })?
            .add_service(control)
            .add_service(management)
            .add_service(session_data)
            .add_service(subagent_control)
            .add_service(tool_images)
            .serve_with_shutdown(self.bind_address, shutdown.cancelled_owned())
            .await
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Serve Agent node control RPC; technical details were withheld.",
                    error,
                )
            })
    }
}

#[derive(Clone)]
pub struct RemoteNodePublisher {
    coordinator: Arc<CoordinatorImpl>,
    nodes: AgentNodeService,
    trusted_node_ca: AgentTlsTrust,
}

impl RemoteNodePublisher {
    async fn publish(
        &self,
        node_id: &str,
        registration: &Registration,
        deployments: &[Deployment],
    ) -> TalosResult<()> {
        let node = self
            .nodes
            .get_node(
                &Actor::regular(&registration.owner_user_id),
                agent_protocol::GetNodeRequest {
                    node_id: node_id.to_owned(),
                },
            )
            .await?;
        let Some(endpoint) = node.endpoint else {
            // Registration alone is not evidence of an outbound execution connection.
            self.unpublish(node_id).await;
            return Ok(());
        };
        let mut clients = Vec::<(String, Arc<dyn AgentHarness>)>::with_capacity(deployments.len());
        for deployment in deployments {
            let client = RemoteAgentClient::connect(
                &endpoint,
                self.trusted_node_ca.clone(),
                deployment.agent_id.clone(),
                NodeAuth {
                    node_id: node_id.to_owned(),
                    token: registration.lease_token.clone(),
                },
            )
            .await?;
            clients.push((deployment.agent_id.clone(), Arc::new(client)));
        }
        self.coordinator.unregister_remote_node(node_id).await;
        for (agent_id, client) in clients {
            self.coordinator
                .replace_remote_agent(node_id.to_owned(), agent_id, client)
                .await?;
        }
        let environment = agent_client::AgentEnvironmentClient::connect(
            &endpoint,
            self.trusted_node_ca.clone(),
            NodeAuth {
                node_id: node_id.to_owned(),
                token: registration.lease_token.clone(),
            },
        )
        .await?;
        self.coordinator
            .register_environment(node_id.to_owned(), Arc::new(environment))
            .await;
        let coordinator = self.coordinator.clone();
        let published_node_id = node_id.to_owned();
        self.nodes
            .mark_registration_published(
                &NodeAuth {
                    node_id: node_id.to_owned(),
                    token: registration.lease_token.clone(),
                },
                Arc::new(move || {
                    let coordinator = coordinator.clone();
                    let node_id = published_node_id.clone();
                    Box::pin(async move {
                        coordinator.unregister_remote_node(&node_id).await;
                    })
                }),
            )
            .await
    }

    async fn unpublish(&self, node_id: &str) {
        self.coordinator.unregister_remote_node(node_id).await;
    }
}

#[derive(Clone)]
pub struct NodeControlRpc {
    nodes: AgentNodeService,
    remote_publisher: RemoteNodePublisher,
    shutdown: CancellationToken,
}

impl NodeControlRpc {
    fn new(
        nodes: AgentNodeService,
        remote_publisher: RemoteNodePublisher,
        shutdown: CancellationToken,
    ) -> Self {
        Self {
            nodes,
            remote_publisher,
            shutdown,
        }
    }
}

#[tonic::async_trait]
impl pb::agent_node_control_server::AgentNodeControl for NodeControlRpc {
    type ConnectStream = std::pin::Pin<
        Box<dyn tokio_stream::Stream<Item = Result<pb::ControlToNodeFrame, Status>> + Send>,
    >;
    async fn connect(
        &self,
        request: Request<tonic::Streaming<pb::NodeToControlFrame>>,
    ) -> Result<Response<Self::ConnectStream>, Status> {
        self.accept_connection(request.into_inner()).await
    }

    async fn register(
        &self,
        request: Request<pb::RegisterRequest>,
    ) -> Result<Response<pb::RegisterResponse>, Status> {
        let result = async {
            let _lifecycle = self.nodes.lock_lifecycle().await;
            let request = request.into_inner();
            let auth = node_pb::decode_auth(request.auth)?;
            let params = node_pb::decode_register(request.params)?;
            let deployments = params.deployments.clone();
            let principal = self.nodes.authenticate_registration(&auth).await?;
            let registration = self.nodes.register(&principal, params).await?;
            self.remote_publisher
                .publish(&auth.node_id, &registration, &deployments)
                .await?;
            Ok(registration)
        }
        .await;
        Ok(Response::new(pb::RegisterResponse {
            result: Some(match result {
                Ok(registration) => pb::register_response::Result::Registered(
                    node_pb::encode_registration(registration),
                ),
                Err(error) => pb::register_response::Result::Failure(failure(error)),
            }),
        }))
    }

    async fn renew_lease(
        &self,
        request: Request<pb::RenewLeaseRequest>,
    ) -> Result<Response<pb::RenewLeaseResponse>, Status> {
        let result = async {
            let request = request.into_inner();
            let auth = node_pb::decode_auth(request.auth)?;
            node_pb::require_empty(request.params)?;
            let principal = self.nodes.authenticate_lease(&auth).await?;
            self.nodes.renew_lease(&principal).await
        }
        .await;
        Ok(Response::new(pb::RenewLeaseResponse {
            result: Some(match result {
                Ok(validity) => {
                    pb::renew_lease_response::Result::Renewed(node_pb::encode_validity(validity))
                }
                Err(error) => pb::renew_lease_response::Result::Failure(failure(error)),
            }),
        }))
    }

    async fn heartbeat(
        &self,
        request: Request<pb::HeartbeatRequest>,
    ) -> Result<Response<pb::HeartbeatResponse>, Status> {
        let result = async {
            let request = request.into_inner();
            let auth = node_pb::decode_auth(request.auth)?;
            let params = node_pb::decode_heartbeat_params(request.params)?;
            let principal = self.nodes.authenticate_lease(&auth).await?;
            self.nodes.heartbeat(&principal, params).await
        }
        .await;
        Ok(Response::new(pb::HeartbeatResponse {
            result: Some(match result {
                Ok(result) => {
                    pb::heartbeat_response::Result::Accepted(node_pb::encode_heartbeat(result))
                }
                Err(error) => pb::heartbeat_response::Result::Failure(failure(error)),
            }),
        }))
    }

    async fn unregister(
        &self,
        request: Request<pb::UnregisterRequest>,
    ) -> Result<Response<pb::UnregisterResponse>, Status> {
        let result = async {
            let _lifecycle = self.nodes.lock_lifecycle().await;
            let request = request.into_inner();
            let auth = node_pb::decode_auth(request.auth)?;
            let node_id = auth.node_id.clone();
            node_pb::require_empty(request.params)?;
            let principal = self.nodes.authenticate_lease(&auth).await?;
            self.nodes.unregister(&principal).await?;
            self.remote_publisher.unpublish(&node_id).await;
            Ok(())
        }
        .await;
        Ok(Response::new(pb::UnregisterResponse {
            result: Some(match result {
                Ok(()) => pb::unregister_response::Result::Completed(pb::Empty {}),
                Err(error) => pb::unregister_response::Result::Failure(failure(error)),
            }),
        }))
    }
}

#[derive(Clone)]
pub struct NodeManagementRpc {
    nodes: AgentNodeService,
    auth_service: Arc<dyn AuthService>,
}

impl NodeManagementRpc {
    fn new(nodes: AgentNodeService, auth_service: Arc<dyn AuthService>) -> Self {
        Self {
            nodes,
            auth_service,
        }
    }

    async fn actor<T>(&self, request: &Request<T>) -> TalosResult<Actor> {
        let token = request
            .metadata()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::Unauthenticated,
                    "User authentication is required",
                )
            })?;
        let identity = self.auth_service.verify_access_token(token).await?;
        if identity.auth_disabled {
            return Err(TalosError::new(
                ErrorCode::Unauthenticated,
                "User authentication is required",
            ));
        }
        Ok(Actor::regular(identity.user_id)
            .with_role(identity.role)
            .with_username(identity.username)
            .with_access_token(token))
    }
}

#[tonic::async_trait]
impl pb::agent_node_management_server::AgentNodeManagement for NodeManagementRpc {
    async fn create_node(
        &self,
        request: Request<pb::CreateNodeRequest>,
    ) -> Result<Response<pb::CreateNodeResponse>, Status> {
        let result = async {
            let actor = self.actor(&request).await?;
            self.nodes
                .create_node(&actor, node_pb::decode_create(request.into_inner()))
                .await
        }
        .await;
        Ok(Response::new(pb::CreateNodeResponse {
            result: Some(match result {
                Ok(created) => {
                    pb::create_node_response::Result::Created(node_pb::encode_created(created))
                }
                Err(error) => pb::create_node_response::Result::Failure(failure(error)),
            }),
        }))
    }

    async fn get_node(
        &self,
        request: Request<pb::GetNodeRequest>,
    ) -> Result<Response<pb::GetNodeResponse>, Status> {
        let result = async {
            let actor = self.actor(&request).await?;
            self.nodes
                .get_node(&actor, node_pb::decode_get(request.into_inner()))
                .await
        }
        .await;
        Ok(Response::new(pb::GetNodeResponse {
            result: Some(match result {
                Ok(info) => pb::get_node_response::Result::Node(node_pb::encode_info(info)),
                Err(error) => pb::get_node_response::Result::Failure(failure(error)),
            }),
        }))
    }

    async fn list_nodes(
        &self,
        request: Request<pb::ListNodesRequest>,
    ) -> Result<Response<pb::ListNodesResponse>, Status> {
        let result = async {
            let actor = self.actor(&request).await?;
            self.nodes
                .list_nodes(&actor, node_pb::decode_list(request.into_inner()))
                .await
        }
        .await;
        Ok(Response::new(pb::ListNodesResponse {
            result: Some(match result {
                Ok(page) => pb::list_nodes_response::Result::Page(node_pb::encode_page(page)),
                Err(error) => pb::list_nodes_response::Result::Failure(failure(error)),
            }),
        }))
    }

    async fn update_node(
        &self,
        request: Request<pb::UpdateNodeRequest>,
    ) -> Result<Response<pb::GetNodeResponse>, Status> {
        let result = async {
            let actor = self.actor(&request).await?;
            let params = node_pb::decode_update(request.into_inner())?;
            self.nodes.update_node(&actor, params).await
        }
        .await;
        Ok(Response::new(pb::GetNodeResponse {
            result: Some(match result {
                Ok(info) => pb::get_node_response::Result::Node(node_pb::encode_info(info)),
                Err(error) => pb::get_node_response::Result::Failure(failure(error)),
            }),
        }))
    }

    async fn rotate_credential(
        &self,
        request: Request<pb::RotateCredentialRequest>,
    ) -> Result<Response<pb::CredentialResponse>, Status> {
        let result = async {
            let actor = self.actor(&request).await?;
            let params = node_pb::decode_rotate(request.into_inner());
            self.nodes.rotate_credential(&actor, params).await
        }
        .await;
        Ok(Response::new(pb::CredentialResponse {
            result: Some(match result {
                Ok(credential) => pb::credential_response::Result::Credential(
                    node_pb::encode_credential(credential),
                ),
                Err(error) => pb::credential_response::Result::Failure(failure(error)),
            }),
        }))
    }
}

fn failure(error: TalosError) -> pb::Failure {
    talos_logging::report_error(&error);
    error_pb::encode_failure(&error)
}

#[cfg(test)]
#[path = "agent_node_rpc_tests.rs"]
mod tests;
