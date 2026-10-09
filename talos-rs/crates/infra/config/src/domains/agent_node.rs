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
}

/// TLS listener serving node registration and authenticated management.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentNodeControlConfig {
    pub bind_address: String,
    pub tls_certificate_file: PathBuf,
    pub tls_private_key_file: PathBuf,
    /// CA used by the control plane when dialing a remote Harness listener.
    pub trusted_node_ca_file: PathBuf,
}

/// Stable identity and TLS endpoints for a standalone Agent node.
///
/// The registration credential is read from a protected file, not from the
/// Workspace or the serialized configuration value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentNodeRunnerConfig {
    pub node_id: String,
    pub control_plane_endpoint: String,
    pub credential_file: PathBuf,
    #[serde(default)]
    pub bind_address: String,
    #[serde(default)]
    pub tls_certificate_file: PathBuf,
    #[serde(default)]
    pub tls_private_key_file: PathBuf,
    pub trusted_ca_file: PathBuf,
}

impl AgentNodeConfig {
    /// Reject node network surfaces without authentication and TLS material.
    pub fn validate(&self, gateway: &GatewayConfig) -> TalosResult<()> {
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
                || control.trusted_node_ca_file.as_os_str().is_empty()
            {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "agent_node.control_plane requires a bind address, TLS identity, and trusted node CA",
                ));
            }
        }
        if let Some(runner) = &self.runner {
            if runner.node_id.trim().is_empty()
                || runner.node_id == "embedded"
                || !runner.control_plane_endpoint.starts_with("https://")
                || runner.credential_file.as_os_str().is_empty()
                || runner.trusted_ca_file.as_os_str().is_empty()
                || runner.bind_address.trim().is_empty()
                || runner.tls_certificate_file.as_os_str().is_empty()
                || runner.tls_private_key_file.as_os_str().is_empty()
            {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "agent_node.runner requires a node ID, HTTPS control endpoint, credential file, trusted CA, bind address, and TLS identity",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "agent_node_tests.rs"]
mod tests;
