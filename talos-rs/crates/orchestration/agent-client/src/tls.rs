use tonic::transport::{Certificate, ClientTlsConfig};

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
