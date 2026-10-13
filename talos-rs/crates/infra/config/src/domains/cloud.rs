use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CloudConfig {
    pub api_base_url: String,
    /// Product and Agent control-plane origin, independent of Cloud Auth.
    pub agent_base_url: String,
    /// Optional private CA for the Agent control plane; system roots are the default.
    pub agent_trusted_ca_file: Option<std::path::PathBuf>,
    pub web_base_url: String,
    pub auth: CloudAuthConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CloudAuthConfig {
    pub introspection_cache_ttl_seconds: u64,
}

impl Default for CloudConfig {
    fn default() -> Self {
        Self {
            api_base_url: "http://127.0.0.1:8787".to_string(),
            agent_base_url: "https://agent.talos.love".to_string(),
            agent_trusted_ca_file: None,
            web_base_url: "http://127.0.0.1:3000".to_string(),
            auth: CloudAuthConfig::default(),
        }
    }
}

impl Default for CloudAuthConfig {
    fn default() -> Self {
        Self {
            introspection_cache_ttl_seconds: 300,
        }
    }
}
