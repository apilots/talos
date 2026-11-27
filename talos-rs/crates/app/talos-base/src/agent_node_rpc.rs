//! Control-plane TLS ingress: WSS/gRPC node lifecycle and existing owner management.

use std::sync::Arc;
use std::{fs, net::SocketAddr};

use agent_client::RemoteAgentClient;
use agent_pb as pb;
use agent_protocol::{error_pb, node_pb};
use talos_auth::AuthService;
use talos_config::AgentNodeControlConfig;
use talos_coordinator::CoordinatorImpl;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::{Actor, AgentNodeService};
use talos_tools::{AgentControl, AutomationControl};
use tokio_util::sync::CancellationToken;

use tonic::{Request, Response, Status};

#[path = "agent_node_resources_rpc.rs"]
mod resources;
pub use resources::{
    NodeAutomationControlRpc, NodeSessionDataRpc, NodeSubagentControlRpc, NodeToolImageRpc,
};
#[path = "agent_node_connect.rs"]
mod connect;
#[path = "agent_desktop_rpc.rs"]
mod desktop;
pub(crate) use desktop::relay::{guard_transport, DesktopRelay};

/// Existing resource owners shared by TLS ingress and in-process Worker bindings.
pub struct NodeResourceServices {
    pub session_data: NodeSessionDataRpc,
    pub agent_control: Arc<dyn AgentControl>,
    pub automation_control: Arc<dyn AutomationControl>,
    pub tool_images: NodeToolImageRpc,
}

pub struct NodeRpcServer {
    bind_address: SocketAddr,
    identity: (Vec<u8>, Vec<u8>),
    nodes: AgentNodeService,
    auth_service: Arc<dyn AuthService>,
    session_data: NodeSessionDataRpc,
    subagent_control: NodeSubagentControlRpc,
    automation_control: NodeAutomationControlRpc,
    tool_images: NodeToolImageRpc,
    remote_publisher: RemoteNodePublisher,
    worker_adapter: Option<agent_client::AgentHarnessRpcAdapter>,
}

impl NodeRpcServer {
    /// Compose the complete node listener with its existing resource owners.
    /// Registration awaits an authenticated WSS or gRPC Connect execution stream.
    /// Callers must supply all resource services before starting the listener.
    pub fn new(
        config: &AgentNodeControlConfig,
        nodes: AgentNodeService,
        auth_service: Arc<dyn AuthService>,
        coordinator: Arc<CoordinatorImpl>,
        resources: NodeResourceServices,
    ) -> TalosResult<Self> {
        let NodeResourceServices {
            session_data,
            agent_control,
            automation_control,
            tool_images,
        } = resources;
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
        let remote_publisher = RemoteNodePublisher {
            coordinator_id: uuid::Uuid::new_v4().to_string(),
            coordinator,
            connections: Arc::default(),
        };
        let subagent_control = NodeSubagentControlRpc::new(session_data.clone(), agent_control);
        let automation_control =
            NodeAutomationControlRpc::new(session_data.clone(), automation_control);
        Ok(Self {
            bind_address,
            identity: (certificate, private_key),
            nodes,
            auth_service,
            session_data,
            subagent_control,
            automation_control,
            tool_images,
            remote_publisher,
            worker_adapter: None,
        })
    }

    /// Publish the same embedded Worker on the central TLS ingress. Its personal
    /// node lease still authorizes every direct request; no extra Worker is built.
    pub fn with_worker_adapter(mut self, adapter: agent_client::AgentHarnessRpcAdapter) -> Self {
        self.worker_adapter = Some(adapter);
        self
    }

    pub fn with_coordinator_id(mut self, coordinator_id: String) -> TalosResult<Self> {
        uuid::Uuid::parse_str(&coordinator_id).map_err(|_| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "Coordinator identity is invalid.",
            )
        })?;
        self.remote_publisher.coordinator_id = coordinator_id;
        Ok(self)
    }

    pub async fn run(self, shutdown: CancellationToken) -> TalosResult<()> {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let node_control = NodeControlRpc::new(
            self.nodes.clone(),
            self.remote_publisher.clone(),
            shutdown.clone(),
        );
        let websocket = node_control.clone().websocket_routes();
        let control = pb::agent_node_control_server::AgentNodeControlServer::new(node_control);
        let management = NodeManagementRpc::new(self.nodes, self.auth_service);
        let desktop = pb::agent_desktop_media_server::AgentDesktopMediaServer::new(
            desktop::DesktopMediaRpc::new(management.clone(), shutdown.clone()),
        )
        .max_decoding_message_size(65 * 1024)
        .max_encoding_message_size(65 * 1024);
        let management =
            pb::agent_node_management_server::AgentNodeManagementServer::new(management);
        let session_data =
            pb::agent_session_data_rpc_server::AgentSessionDataRpcServer::new(self.session_data);
        let subagent_control =
            pb::agent_subagent_control_rpc_server::AgentSubagentControlRpcServer::new(
                self.subagent_control,
            );
        let tool_images =
            pb::agent_tool_image_rpc_server::AgentToolImageRpcServer::new(self.tool_images);
        let automation_control =
            pb::agent_automation_control_rpc_server::AgentAutomationControlRpcServer::new(
                self.automation_control,
            );
        let mut routes = tonic::service::Routes::new(control)
            .add_service(management)
            .add_service(desktop)
            .add_service(session_data)
            .add_service(subagent_control)
            .add_service(automation_control)
            .add_service(tool_images);
        if let Some(adapter) = self.worker_adapter {
            routes = routes
                .add_service(
                    pb::worker_environment_rpc_server::WorkerEnvironmentRpcServer::new(
                        adapter.clone(),
                    ),
                )
                .add_service(pb::agent_harness_rpc_server::AgentHarnessRpcServer::new(
                    adapter,
                ));
        }
        let router = node_control_router(routes, websocket);
        let tls = axum_server::tls_rustls::RustlsConfig::from_pem(self.identity.0, self.identity.1)
            .await
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Configure Agent node TLS; technical details were withheld.",
                    error,
                )
            })?;
        let handle = axum_server::Handle::new();
        let signal = handle.clone();
        let server = axum_server::bind_rustls(self.bind_address, tls)
            .handle(handle)
            .serve(router.into_make_service());
        tokio::pin!(server);
        tokio::select! {
            result = &mut server => result,
            _ = shutdown.cancelled() => {
                signal.graceful_shutdown(Some(std::time::Duration::from_secs(10)));
                server.await
            }
        }
        .map_err(|error| {
            TalosError::internal_with_source(
                "Serve Agent node control; technical details were withheld.",
                error,
            )
        })
    }
}

fn node_control_router(
    routes: tonic::service::Routes,
    websocket: axum_ws::Router,
) -> axum_ws::Router {
    routes
        .into_axum_router()
        .layer(axum_ws::middleware::from_fn(honor_grpc_timeout))
        .merge(websocket)
}

async fn honor_grpc_timeout(
    request: axum_ws::extract::Request,
    next: axum_ws::middleware::Next,
) -> axum_ws::response::Response {
    match grpc_timeout(request.headers()) {
        Some(deadline) => match tokio::time::timeout(deadline, next.run(request)).await {
            Ok(response) => response,
            Err(_) => Status::cancelled("Timeout expired").into_http(),
        },
        None => next.run(request).await,
    }
}

fn grpc_timeout(headers: &axum_ws::http::HeaderMap) -> Option<std::time::Duration> {
    let value = headers.get("grpc-timeout")?.to_str().ok()?;
    if value.is_empty() {
        return None;
    }
    let (amount, unit) = value.split_at(value.len() - 1);
    if amount.is_empty() || amount.len() > 8 {
        return None;
    }
    let amount = amount.parse().ok()?;
    Some(match unit {
        "H" => std::time::Duration::from_secs(amount * 60 * 60),
        "M" => std::time::Duration::from_secs(amount * 60),
        "S" => std::time::Duration::from_secs(amount),
        "m" => std::time::Duration::from_millis(amount),
        "u" => std::time::Duration::from_micros(amount),
        "n" => std::time::Duration::from_nanos(amount),
        _ => return None,
    })
}

#[derive(Clone)]
pub struct RemoteNodePublisher {
    coordinator_id: String,
    coordinator: Arc<CoordinatorImpl>,
    // Live transport handles belong to the existing publication owner; no
    // additional persisted registry or execution ledger is introduced.
    connections: Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<String, Arc<agent_client::AgentEnvironmentClient>>,
        >,
    >,
}

impl RemoteNodePublisher {
    async fn unpublish(&self, node_id: &str) {
        if let Some(connection) = self.connections.lock().await.remove(node_id) {
            connection.close();
        }
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
            let request = request.into_inner();
            let auth = node_pb::decode_auth(request.auth)?;
            let _lifecycle = self.nodes.lock_lifecycle(&auth.node_id).await;
            let params = node_pb::decode_register(request.params)?;
            let principal = self.nodes.authenticate_registration(&auth).await?;
            let registration = self.nodes.register(&principal, params).await?;
            self.remote_publisher.unpublish(&auth.node_id).await;
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
            let request = request.into_inner();
            let auth = node_pb::decode_auth(request.auth)?;
            let _lifecycle = self.nodes.lock_lifecycle(&auth.node_id).await;
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
            .with_user_type(identity.user_type)
            .with_username(identity.username)
            .with_access_token(token))
    }
}

#[tonic::async_trait]
impl pb::agent_node_management_server::AgentNodeManagement for NodeManagementRpc {
    async fn delete_computer(
        &self,
        request: Request<pb::GetNodeRequest>,
    ) -> Result<Response<pb::DeleteComputerResponse>, Status> {
        let result = async {
            let actor = self.actor(&request).await?;
            self.nodes
                .delete_computer(
                    &actor,
                    talos_types::DeleteComputerRequest {
                        node_id: request.into_inner().node_id,
                    },
                )
                .await
        }
        .await;
        Ok(Response::new(pb::DeleteComputerResponse {
            result: Some(match result {
                Ok(value) => pb::delete_computer_response::Result::NodeId(value.node_id),
                Err(error) => pb::delete_computer_response::Result::Failure(failure(error)),
            }),
        }))
    }

    async fn get_deployments(
        &self,
        request: Request<pb::GetNodeRequest>,
    ) -> Result<Response<pb::GetDeploymentsResponse>, Status> {
        let result = async {
            let actor = self.actor(&request).await?;
            self.nodes
                .owned_deployments(&actor, &request.into_inner().node_id)
                .await
        }
        .await;
        Ok(Response::new(pb::GetDeploymentsResponse {
            result: Some(match result {
                Ok(deployments) => {
                    pb::get_deployments_response::Result::Deployments(pb::NodeDeployments {
                        deployments: deployments
                            .into_iter()
                            .map(|deployment| pb::Deployment {
                                agent_id: deployment.agent_id,
                            })
                            .collect(),
                    })
                }
                Err(error) => pb::get_deployments_response::Result::Failure(failure(error)),
            }),
        }))
    }

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
                Ok(created) => pb::create_node_response::Result::Created(Box::new(
                    node_pb::encode_created(created),
                )),
                Err(error) => pb::create_node_response::Result::Failure(Box::new(failure(error))),
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
                Ok(info) => {
                    pb::get_node_response::Result::Node(Box::new(node_pb::encode_info(info)))
                }
                Err(error) => pb::get_node_response::Result::Failure(Box::new(failure(error))),
            }),
        }))
    }

    async fn verify_saved_registration(
        &self,
        request: Request<pb::VerifySavedRegistrationRequest>,
    ) -> Result<Response<pb::GetNodeResponse>, Status> {
        let result = async {
            let actor = self.actor(&request).await?;
            let auth = node_pb::decode_auth(request.into_inner().auth)?;
            self.nodes.verify_saved_registration(&actor, &auth).await
        }
        .await;
        Ok(Response::new(pb::GetNodeResponse {
            result: Some(match result {
                Ok(node) => {
                    pb::get_node_response::Result::Node(Box::new(node_pb::encode_info(node)))
                }
                Err(error) => pb::get_node_response::Result::Failure(Box::new(failure(error))),
            }),
        }))
    }

    async fn list_nodes(
        &self,
        request: Request<pb::ListNodesRequest>,
    ) -> Result<Response<pb::ListNodesResponse>, Status> {
        let result = async {
            let actor = self.actor(&request).await?;
            let request = request.into_inner();
            if request.owner_only {
                let page = self
                    .nodes
                    .list_computers(
                        &actor,
                        talos_types::ComputerListRequest {
                            cursor: request.cursor,
                            limit: Some(request.limit),
                            query: None,
                        },
                    )
                    .await?;
                return Ok(talos_types::NodePage {
                    data: page.data,
                    next_cursor: page.next_cursor,
                });
            }
            self.nodes
                .list_nodes(&actor, node_pb::decode_list(request))
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

    async fn rename_computer(
        &self,
        request: Request<pb::RenameComputerRequest>,
    ) -> Result<Response<pb::GetNodeResponse>, Status> {
        let result = async {
            let actor = self.actor(&request).await?;
            let request = request.into_inner();
            self.nodes
                .rename_computer(
                    &actor,
                    talos_types::RenameComputerRequest {
                        node_id: request.node_id,
                        display_name: request.display_name,
                    },
                )
                .await
        }
        .await;
        Ok(Response::new(pb::GetNodeResponse {
            result: Some(match result {
                Ok(info) => {
                    pb::get_node_response::Result::Node(Box::new(node_pb::encode_info(info)))
                }
                Err(error) => pb::get_node_response::Result::Failure(Box::new(failure(error))),
            }),
        }))
    }
    async fn set_computer_status(
        &self,
        request: Request<pb::SetComputerStatusRequest>,
    ) -> Result<Response<pb::GetNodeResponse>, Status> {
        let result = async {
            let actor = self.actor(&request).await?;
            let request = request.into_inner();
            self.nodes
                .set_computer_status(
                    &actor,
                    talos_types::SetComputerStatusRequest {
                        node_id: request.node_id,
                        status: node_pb::decode_status(request.status)?,
                    },
                )
                .await
        }
        .await;
        Ok(Response::new(pb::GetNodeResponse {
            result: Some(match result {
                Ok(info) => {
                    pb::get_node_response::Result::Node(Box::new(node_pb::encode_info(info)))
                }
                Err(error) => pb::get_node_response::Result::Failure(Box::new(failure(error))),
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
                Ok(info) => {
                    pb::get_node_response::Result::Node(Box::new(node_pb::encode_info(info)))
                }
                Err(error) => pb::get_node_response::Result::Failure(Box::new(failure(error))),
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
