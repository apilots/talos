use tonic::transport::{Certificate, ClientTlsConfig};

pub(crate) async fn loopback_channel(
    address: std::net::SocketAddr,
) -> talos_error::TalosResult<tonic::transport::Channel> {
    use talos_error::{ErrorCode, TalosError};
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Local Worker RPC requires a bound loopback socket",
        ));
    }
    tonic::transport::Endpoint::from_shared(format!("http://{address}"))
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::InvalidRequest,
                "Local Worker endpoint is invalid",
                error,
            )
        })?
        .connect_timeout(std::time::Duration::from_secs(10))
        .connect()
        .await
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::ServiceUnavailable,
                "Local Worker RPC is unavailable",
                error,
            )
        })
}

/// Server certificate trust shared by node, resource and Harness clients.
/// System roots retain hostname verification; private deployments supply a CA.
#[derive(Clone)]
pub enum AgentTlsTrust {
    SystemRoots,
    Certificate(Vec<u8>),
}

impl AgentTlsTrust {
    pub(crate) fn into_config(self) -> ClientTlsConfig {
        match self {
            Self::SystemRoots => ClientTlsConfig::new().with_native_roots(),
            Self::Certificate(pem) => {
                ClientTlsConfig::new().ca_certificate(Certificate::from_pem(pem))
            }
        }
    }
}
