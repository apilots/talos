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
    /// Omitted on existing listener configurations preserves direct deployment.
    #[serde(default)]
    pub transport: Option<AgentNodeTransport>,
    pub node_id: String,
    pub control_plane_endpoint: String,
    pub credential_file: PathBuf,
    #[serde(default)]
    pub bind_address: String,
    #[serde(default)]
    pub tls_certificate_file: PathBuf,
    #[serde(default)]
    pub tls_private_key_file: PathBuf,
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
        self.transport.unwrap_or_else(|| {
            if self.bind_address.is_empty()
                && self.tls_certificate_file.as_os_str().is_empty()
                && self.tls_private_key_file.as_os_str().is_empty()
            {
                AgentNodeTransport::Outbound
            } else {
                AgentNodeTransport::Direct
            }
        })
    }
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
                    && (runner.bind_address.trim().is_empty()
                        || runner.tls_certificate_file.as_os_str().is_empty()
                        || runner.tls_private_key_file.as_os_str().is_empty()))
                || (runner.transport() == AgentNodeTransport::Outbound
                    && (!runner.bind_address.is_empty()
                        || !runner.tls_certificate_file.as_os_str().is_empty()
                        || !runner.tls_private_key_file.as_os_str().is_empty()))
                || runner
                    .trusted_ca_file
                    .as_ref()
                    .is_some_and(|path| path.as_os_str().is_empty())
            {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "agent_node.runner requires a node ID, HTTPS control endpoint, credential file, and transport-specific TLS settings",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "agent_node_tests.rs"]
mod tests;
