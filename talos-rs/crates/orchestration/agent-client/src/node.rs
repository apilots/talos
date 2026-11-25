use agent_pb as pb;
use agent_protocol::{
    error_pb, node_pb, HeartbeatParams, HeartbeatResult, LeaseValidity, NodeRequest,
    RegisterParams, Registration,
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use tonic::transport::{Channel, Endpoint};

use crate::AgentTlsTrust;

/// Concrete node lifecycle client; WSS personal Workers and explicit gRPC cloud Workers.
/// Management methods retain their existing gRPC channel.
#[derive(Clone)]
pub struct AgentNodeClient {
    pub(crate) channel: Channel,
    websocket: Option<(String, reqwest::Client)>,
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
        Ok(Self {
            channel,
            websocket: None,
        })
    }

    /// Use WSS for node lifecycle and Connect, without transport fallback.
    /// Actor enrollment/management remains on the existing HTTPS endpoint.
    pub async fn connect_websocket(endpoint: &str, trust: AgentTlsTrust) -> TalosResult<Self> {
        let mut client = Self::connect(endpoint, trust.clone()).await?;
        let mut builder = reqwest::Client::builder()
            .http1_only()
            .connect_timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none());
        if let AgentTlsTrust::Certificate(pem) = trust {
            let certificates = reqwest::Certificate::from_pem_bundle(&pem).map_err(|error| {
                TalosError::from_source(
                    ErrorCode::InvalidRequest,
                    "Agent node CA is invalid",
                    error,
                )
            })?;
            builder = builder.tls_built_in_root_certs(false);
            for certificate in certificates {
                builder = builder.add_root_certificate(certificate);
            }
        }
        let http = builder.build().map_err(|error| {
            TalosError::internal_with_source(
                "Configure Agent WebSocket TLS; technical details were withheld.",
                error,
            )
        })?;
        let base = endpoint.replacen("https://", "wss://", 1);
        client.websocket = Some((base.trim_end_matches('/').to_owned(), http));
        Ok(client)
    }

    pub(crate) fn uses_websocket(&self) -> bool {
        self.websocket.is_some()
    }

    pub(crate) async fn open_websocket(
        &self,
        method: &str,
    ) -> TalosResult<reqwest_websocket::WebSocket> {
        use reqwest_websocket::RequestBuilderExt;
        let (base, client) = self
            .websocket
            .as_ref()
            .ok_or_else(|| TalosError::internal("Agent WSS transport is not configured"))?;
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            client
                .get(format!("{base}/talos/worker/{method}"))
                .upgrade()
                .web_socket_config(
                    tungstenite::protocol::WebSocketConfig::default()
                        .max_message_size(Some(super::worker_connection::MAX_FRAME_BYTES))
                        .max_frame_size(Some(super::worker_connection::MAX_FRAME_BYTES)),
                )
                .send()
                .await?
                .into_websocket()
                .await
        })
        .await
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::Timeout,
                "Agent WebSocket handshake timed out",
                error,
            )
        })?
        .map_err(websocket_error)
    }

    async fn websocket_unary<T: prost::Message, R: prost::Message + Default>(
        &self,
        method: &str,
        request: T,
    ) -> TalosResult<R> {
        use futures_util::{SinkExt, StreamExt};
        if request.encoded_len() > super::worker_connection::MAX_FRAME_BYTES {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Agent WebSocket request exceeds the transport limit",
            ));
        }
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let mut socket = self.open_websocket(method).await?;
            socket
                .send(reqwest_websocket::Message::Binary(
                    request.encode_to_vec().into(),
                ))
                .await
                .map_err(websocket_error)?;
            while let Some(message) = socket.next().await {
                match message.map_err(websocket_error)? {
                    reqwest_websocket::Message::Binary(bytes) => {
                        return R::decode(bytes).map_err(|error| {
                            TalosError::internal_with_source(
                                "Agent WebSocket response is malformed",
                                error,
                            )
                        })
                    }
                    reqwest_websocket::Message::Ping(_) | reqwest_websocket::Message::Pong(_) => {
                        continue
                    }
                    _ => return Err(invalid_response()),
                }
            }
            Err(super::connection_writer::closed())
        })
        .await
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::Timeout,
                "Agent node WebSocket request timed out",
                error,
            )
        })?
    }

    pub async fn register(
        &self,
        request: NodeRequest<RegisterParams>,
    ) -> TalosResult<Registration> {
        let request = pb::RegisterRequest {
            auth: Some(node_pb::encode_auth(request.auth)),
            params: Some(node_pb::encode_register(request.params)),
        };
        let response: pb::RegisterResponse = if self.uses_websocket() {
            self.websocket_unary("register", request).await?
        } else {
            pb::agent_node_control_client::AgentNodeControlClient::new(self.channel.clone())
                .register(node_request(request))
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner()
        };
        match response.result.ok_or_else(invalid_response)? {
            pb::register_response::Result::Registered(value) => node_pb::decode_registration(value),
            pb::register_response::Result::Failure(value) => Err(error_pb::decode_failure(value)?),
        }
    }

    pub async fn renew_lease(&self, request: NodeRequest<()>) -> TalosResult<LeaseValidity> {
        let request = pb::RenewLeaseRequest {
            auth: Some(node_pb::encode_auth(request.auth)),
            params: Some(pb::Empty {}),
        };
        let response: pb::RenewLeaseResponse = if self.uses_websocket() {
            self.websocket_unary("renew_lease", request).await?
        } else {
            pb::agent_node_control_client::AgentNodeControlClient::new(self.channel.clone())
                .renew_lease(node_request(request))
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner()
        };
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
        let request = pb::HeartbeatRequest {
            auth: Some(node_pb::encode_auth(request.auth)),
            params: Some(pb::HeartbeatParams {
                health: node_pb::encode_health(request.params.health),
            }),
        };
        let response: pb::HeartbeatResponse = if self.uses_websocket() {
            self.websocket_unary("heartbeat", request).await?
        } else {
            pb::agent_node_control_client::AgentNodeControlClient::new(self.channel.clone())
                .heartbeat(node_request(request))
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner()
        };
        match response.result.ok_or_else(invalid_response)? {
            pb::heartbeat_response::Result::Accepted(value) => Ok(node_pb::decode_heartbeat(value)),
            pb::heartbeat_response::Result::Failure(value) => Err(error_pb::decode_failure(value)?),
        }
    }

    pub async fn unregister(&self, request: NodeRequest<()>) -> TalosResult<()> {
        let request = pb::UnregisterRequest {
            auth: Some(node_pb::encode_auth(request.auth)),
            params: Some(pb::Empty {}),
        };
        let response: pb::UnregisterResponse = if self.uses_websocket() {
            self.websocket_unary("unregister", request).await?
        } else {
            pb::agent_node_control_client::AgentNodeControlClient::new(self.channel.clone())
                .unregister(node_request(request))
                .await
                .map_err(crate::decode_rpc_status)?
                .into_inner()
        };
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

pub(crate) fn websocket_error(error: reqwest_websocket::Error) -> TalosError {
    TalosError::from_source(
        ErrorCode::ServiceUnavailable,
        "Agent WebSocket connection failed",
        error,
    )
}
