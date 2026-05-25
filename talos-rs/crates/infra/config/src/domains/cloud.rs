use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CloudConfig {
    pub enabled: bool,
    pub api_base_url: String,
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
            enabled: false,
            api_base_url: "http://127.0.0.1:8787".to_string(),
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
