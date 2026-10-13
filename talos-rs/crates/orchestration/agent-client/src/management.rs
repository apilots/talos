use crate::AgentNodeClient;
use agent_pb as pb;
use agent_protocol::{
    error_pb, node_pb, CreateNodeRequest, CreatedNode, NodeCredential, NodeInfo,
    RotateCredentialRequest,
};
use talos_error::{ErrorCode, TalosError, TalosResult};

impl AgentNodeClient {
    /// Discover the center's current advertisements under the logged-in owner.
    /// No lease credential or second node registration is returned.
    pub async fn get_deployments(
        &self,
        access_token: &str,
        node_id: &str,
    ) -> TalosResult<Vec<agent_protocol::Deployment>> {
        let response =
            pb::agent_node_management_client::AgentNodeManagementClient::new(self.channel.clone())
                .get_deployments(actor_request(
                    access_token,
                    pb::GetNodeRequest {
                        node_id: node_id.into(),
                    },
                )?)
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        match response.result.ok_or_else(malformed)? {
            pb::get_deployments_response::Result::Deployments(value) => {
                let params = node_pb::decode_register(Some(pb::RegisterParams {
                    system_info: None,
                    deployments: value.deployments,
                }))?;
                Ok(params.deployments)
            }
            pb::get_deployments_response::Result::Failure(value) => {
                Err(error_pb::decode_failure(value)?)
            }
        }
    }

    /// Actor-authenticated enrollment. Node credentials never authorize management.
    pub async fn create_node(
        &self,
        access_token: &str,
        request: CreateNodeRequest,
    ) -> TalosResult<CreatedNode> {
        let response =
            pb::agent_node_management_client::AgentNodeManagementClient::new(self.channel.clone())
                .create_node(actor_request(
                    access_token,
                    pb::CreateNodeRequest {
                        display_name: request.display_name,
                        endpoint: request.endpoint,
                        owner_user_id: request.owner_user_id,
                        credential_expires_at: request.credential_expires_at,
                    },
                )?)
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        match response.result.ok_or_else(malformed)? {
            pb::create_node_response::Result::Created(value) => node_pb::decode_created(value),
            pb::create_node_response::Result::Failure(value) => {
                Err(error_pb::decode_failure(value)?)
            }
        }
    }

    pub async fn get_node(&self, access_token: &str, node_id: &str) -> TalosResult<NodeInfo> {
        let response =
            pb::agent_node_management_client::AgentNodeManagementClient::new(self.channel.clone())
                .get_node(actor_request(
                    access_token,
                    pb::GetNodeRequest {
                        node_id: node_id.into(),
                    },
                )?)
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        match response.result.ok_or_else(malformed)? {
            pb::get_node_response::Result::Node(value) => node_pb::decode_info(value),
            pb::get_node_response::Result::Failure(value) => Err(error_pb::decode_failure(value)?),
        }
    }

    pub async fn update_node(
        &self,
        access_token: &str,
        request: talos_types::UpdateNodeRequest,
    ) -> TalosResult<NodeInfo> {
        let response =
            pb::agent_node_management_client::AgentNodeManagementClient::new(self.channel.clone())
                .update_node(actor_request(
                    access_token,
                    pb::UpdateNodeRequest {
                        node_id: request.node_id,
                        display_name: request.display_name,
                        endpoint: request.endpoint,
                        status: node_pb::encode_status(request.status),
                    },
                )?)
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        decode_node(response)
    }

    pub async fn rename_computer(
        &self,
        access_token: &str,
        request: talos_types::RenameComputerRequest,
    ) -> TalosResult<NodeInfo> {
        let response =
            pb::agent_node_management_client::AgentNodeManagementClient::new(self.channel.clone())
                .rename_computer(actor_request(
                    access_token,
                    pb::RenameComputerRequest {
                        node_id: request.node_id,
                        display_name: request.display_name,
                    },
                )?)
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        decode_node(response)
    }
    pub async fn set_computer_status(
        &self,
        access_token: &str,
        request: talos_types::SetComputerStatusRequest,
    ) -> TalosResult<NodeInfo> {
        let response =
            pb::agent_node_management_client::AgentNodeManagementClient::new(self.channel.clone())
                .set_computer_status(actor_request(
                    access_token,
                    pb::SetComputerStatusRequest {
                        node_id: request.node_id,
                        status: node_pb::encode_status(request.status),
                    },
                )?)
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        decode_node(response)
    }

    pub async fn rotate_credential(
        &self,
        access_token: &str,
        request: RotateCredentialRequest,
    ) -> TalosResult<NodeCredential> {
        let response =
            pb::agent_node_management_client::AgentNodeManagementClient::new(self.channel.clone())
                .rotate_credential(actor_request(
                    access_token,
                    pb::RotateCredentialRequest {
                        node_id: request.node_id,
                        expires_at: request.expires_at,
                    },
                )?)
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        match response.result.ok_or_else(malformed)? {
            pb::credential_response::Result::Credential(value) => node_pb::decode_credential(value),
            pb::credential_response::Result::Failure(value) => {
                Err(error_pb::decode_failure(value)?)
            }
        }
    }
}

fn actor_request<T>(token: &str, message: T) -> TalosResult<tonic::Request<T>> {
    if token.is_empty() {
        return Err(TalosError::new(
            ErrorCode::Unauthenticated,
            "User authentication is required",
        ));
    }
    let mut request = tonic::Request::new(message);
    let value = format!("Bearer {token}").parse().map_err(|_| {
        TalosError::new(
            ErrorCode::Unauthenticated,
            "User authentication token is invalid",
        )
    })?;
    request.set_timeout(std::time::Duration::from_secs(10));
    request.metadata_mut().insert("authorization", value);
    Ok(request)
}

fn malformed() -> TalosError {
    TalosError::internal("Agent node management response is malformed")
}

fn decode_node(response: pb::GetNodeResponse) -> TalosResult<NodeInfo> {
    match response.result.ok_or_else(malformed)? {
        pb::get_node_response::Result::Node(value) => node_pb::decode_info(value),
        pb::get_node_response::Result::Failure(value) => Err(error_pb::decode_failure(value)?),
    }
}
