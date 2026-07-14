use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use talos_config::{
    LlmConfig, ModelReadiness, ProviderKeyStatus, ProviderProtocolCapabilities,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ConfigGetRequest {
    #[serde(default)]
    #[ts(optional)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct ConfigGetResponse {
    pub config: LlmConfig,
    pub model_readiness: ModelReadiness,
    pub provider_keys: BTreeMap<String, ProviderKeyStatus>,
    pub provider_capabilities: BTreeMap<String, ProviderProtocolCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct ConfigUpdateRequest {
    pub path: String,
    pub value: ConfigValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct ConfigUpdateResponse {
    pub config: LlmConfig,
    pub model_readiness: ModelReadiness,
    pub provider_keys: BTreeMap<String, ProviderKeyStatus>,
    pub provider_capabilities: BTreeMap<String, ProviderProtocolCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ConfigDeleteRequest {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ConfigDeleteResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ConfigReloadRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ConfigReloadResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ConfigValue {
    Llm(LlmConfig),
}

impl From<talos_config::LlmConfigRead> for ConfigGetResponse {
    fn from(value: talos_config::LlmConfigRead) -> Self {
        Self {
            config: value.config,
            model_readiness: value.model_readiness,
            provider_keys: value.provider_keys,
            provider_capabilities: talos_config::provider_protocol_capabilities(),
        }
    }
}

impl From<talos_config::LlmConfigRead> for ConfigUpdateResponse {
    fn from(value: talos_config::LlmConfigRead) -> Self {
        let response = ConfigGetResponse::from(value);
        Self {
            config: response.config,
            model_readiness: response.model_readiness,
            provider_keys: response.provider_keys,
            provider_capabilities: response.provider_capabilities,
        }
    }
}
