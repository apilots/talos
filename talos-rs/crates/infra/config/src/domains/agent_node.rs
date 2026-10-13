use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use talos_error::{ErrorCode, TalosError, TalosResult};

use super::network::GatewayConfig;

/// Network configuration for the control plane and a standalone Agent node.
/// Neither section is required for the existing embedded-only server.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentNodeConfig {
    pub control_plane: Option<AgentNodeControlConfig>,
    pub runner: Option<AgentNodeRunnerConfig>,
    pub worker_rpc: WorkerRpcConfig,
}

/// The execution listener is independent of registration and Connect transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkerRpcConfig {
    pub bind_address: String,
    pub tls: Option<WorkerRpcTlsConfig>,
}

impl Default for WorkerRpcConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:0".into(),
            tls: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRpcTlsConfig {
    pub certificate_file: PathBuf,
    pub private_key_file: PathBuf,
}

impl WorkerRpcConfig {
    /// Plain HTTP/2 is restricted to an explicit loopback socket, with NodeAuth.
    pub fn address(&self) -> TalosResult<std::net::SocketAddr> {
        let address: std::net::SocketAddr = self.bind_address.parse().map_err(|error| {
            TalosError::from_source(
                ErrorCode::InvalidRequest,
                "Worker RPC bind address is invalid",
                error,
            )
        })?;
        if (!address.ip().is_loopback() && self.tls.is_none())
            || self.tls.as_ref().is_some_and(|tls| {
                tls.certificate_file.as_os_str().is_empty()
                    || tls.private_key_file.as_os_str().is_empty()
            })
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Non-loopback Worker RPC requires a TLS identity",
            ));
        }
        Ok(address)
    }
}

/// TLS listener serving node registration and authenticated management.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentNodeControlConfig {
    pub bind_address: String,
    pub tls_certificate_file: PathBuf,
    pub tls_private_key_file: PathBuf,
    /// Optional private CA for direct Harness listeners; omitted uses system roots.
    #[serde(default)]
    pub trusted_node_ca_file: Option<PathBuf>,
}

/// Stable identity and TLS endpoints for a standalone Agent node.
///
/// The registration credential is read from a protected file, not from the
/// Workspace or the serialized configuration value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentNodeRunnerConfig {
    /// Omitted uses outbound Connect; listener settings never select registration transport.
    #[serde(default)]
    pub transport: Option<AgentNodeTransport>,
    pub node_id: String,
    pub control_plane_endpoint: String,
    pub credential_file: PathBuf,
    /// Optional private CA for the control plane; omitted uses system roots.
    #[serde(default)]
    pub trusted_ca_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentNodeTransport {
    Outbound,
    Direct,
}

impl AgentNodeRunnerConfig {
    pub fn transport(&self) -> AgentNodeTransport {
        self.transport.unwrap_or(AgentNodeTransport::Outbound)
    }
}

impl AgentNodeConfig {
    /// Reject node network surfaces without authentication and TLS material.
    pub fn validate(&self, gateway: &GatewayConfig) -> TalosResult<()> {
        self.worker_rpc.address()?;
        if let Some(control) = &self.control_plane {
            if !gateway.auth {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "agent_node.control_plane requires gateway.auth=true",
                ));
            }
            if control.bind_address.trim().is_empty()
                || control.tls_certificate_file.as_os_str().is_empty()
                || control.tls_private_key_file.as_os_str().is_empty()
                || control
                    .trusted_node_ca_file
                    .as_ref()
                    .is_some_and(|path| path.as_os_str().is_empty())
            {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "agent_node.control_plane requires a bind address, TLS identity",
                ));
            }
        }
        if let Some(runner) = &self.runner {
            if runner.node_id.trim().is_empty()
                || runner.node_id == "embedded"
                || !runner.control_plane_endpoint.starts_with("https://")
                || runner.credential_file.as_os_str().is_empty()
                || (runner.transport() == AgentNodeTransport::Direct
                    && self.worker_rpc.tls.is_none())
                || runner
                    .trusted_ca_file
                    .as_ref()
                    .is_some_and(|path| path.as_os_str().is_empty())
            {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Agent node runner requires a node ID, HTTPS control endpoint, credential file, and TLS for direct access",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "agent_node_tests.rs"]
mod tests;
