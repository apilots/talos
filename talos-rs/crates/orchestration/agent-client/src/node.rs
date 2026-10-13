use agent_pb as pb;
use agent_protocol::{
    error_pb, node_pb, HeartbeatParams, HeartbeatResult, LeaseValidity, NodeRequest,
    RegisterParams, Registration,
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use tonic::transport::{Channel, Endpoint};

use crate::AgentTlsTrust;

/// Concrete gRPC client for control-plane node registration and lease methods.
#[derive(Clone)]
pub struct AgentNodeClient {
    pub(crate) channel: Channel,
}

impl AgentNodeClient {
    pub async fn connect(endpoint: &str, trust: AgentTlsTrust) -> TalosResult<Self> {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        if !endpoint.starts_with("https://") {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Agent node control endpoint must use TLS",
            ));
        }
        let endpoint = Endpoint::from_shared(endpoint.to_string()).map_err(|error| {
            TalosError::from_source(
                ErrorCode::InvalidRequest,
                "Agent node control endpoint is invalid",
                error,
            )
        })?;
        let endpoint = endpoint
            .connect_timeout(std::time::Duration::from_secs(10))
            .tls_config(trust.into_config())
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Configure Agent node TLS; technical details were withheld.",
                    error,
                )
            })?;
        let channel = endpoint.connect_lazy();
        Ok(Self { channel })
    }

    pub async fn register(
        &self,
        request: NodeRequest<RegisterParams>,
    ) -> TalosResult<Registration> {
        let response =
            pb::agent_node_control_client::AgentNodeControlClient::new(self.channel.clone())
                .register(node_request(pb::RegisterRequest {
                    auth: Some(node_pb::encode_auth(request.auth)),
                    params: Some(node_pb::encode_register(request.params)),
                }))
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        match response.result.ok_or_else(invalid_response)? {
            pb::register_response::Result::Registered(value) => node_pb::decode_registration(value),
            pb::register_response::Result::Failure(value) => Err(error_pb::decode_failure(value)?),
        }
    }

    pub async fn renew_lease(&self, request: NodeRequest<()>) -> TalosResult<LeaseValidity> {
        let response =
            pb::agent_node_control_client::AgentNodeControlClient::new(self.channel.clone())
                .renew_lease(node_request(pb::RenewLeaseRequest {
                    auth: Some(node_pb::encode_auth(request.auth)),
                    params: Some(pb::Empty {}),
                }))
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        match response.result.ok_or_else(invalid_response)? {
            pb::renew_lease_response::Result::Renewed(value) => node_pb::decode_validity(value),
            pb::renew_lease_response::Result::Failure(value) => {
                Err(error_pb::decode_failure(value)?)
            }
        }
    }

    pub async fn heartbeat(
        &self,
        request: NodeRequest<HeartbeatParams>,
    ) -> TalosResult<HeartbeatResult> {
        let response =
            pb::agent_node_control_client::AgentNodeControlClient::new(self.channel.clone())
                .heartbeat(node_request(pb::HeartbeatRequest {
                    auth: Some(node_pb::encode_auth(request.auth)),
                    params: Some(pb::HeartbeatParams {
                        health: node_pb::encode_health(request.params.health),
                    }),
                }))
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        match response.result.ok_or_else(invalid_response)? {
            pb::heartbeat_response::Result::Accepted(value) => Ok(node_pb::decode_heartbeat(value)),
            pb::heartbeat_response::Result::Failure(value) => Err(error_pb::decode_failure(value)?),
        }
    }

    pub async fn unregister(&self, request: NodeRequest<()>) -> TalosResult<()> {
        let response =
            pb::agent_node_control_client::AgentNodeControlClient::new(self.channel.clone())
                .unregister(node_request(pb::UnregisterRequest {
                    auth: Some(node_pb::encode_auth(request.auth)),
                    params: Some(pb::Empty {}),
                }))
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner();
        match response.result.ok_or_else(invalid_response)? {
            pb::unregister_response::Result::Completed(_) => Ok(()),
            pb::unregister_response::Result::Failure(value) => {
                Err(error_pb::decode_failure(value)?)
            }
        }
    }
}

fn invalid_response() -> TalosError {
    TalosError::internal("Agent node RPC response is malformed")
}

fn node_request<T>(body: T) -> tonic::Request<T> {
    let mut request = tonic::Request::new(body);
    request.set_timeout(std::time::Duration::from_secs(10));
    request
}
