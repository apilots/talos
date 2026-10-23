use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use talos_config::{
    LlmConfig, ModelReadiness, ProviderKeyStatus, ProviderProtocolCapabilities,
    WebSearchProviderConfig, WebSearchProviderRuntimeConfig, WebSearchToolConfig,
};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ConfigPath {
    #[default]
    Llm,
    WebSearch,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ConfigGetRequest {
    pub node_id: Option<String>,
    #[serde(default)]
    pub path: ConfigPath,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ConfigUpdateRequest {
    pub node_id: Option<String>,
    pub value: ConfigValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ConfigDeleteRequest {
    pub node_id: Option<String>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ConfigDeleteResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct ConfigReloadRequest {
    pub node_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ConfigReloadResponse {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ConfigValue {
    Llm(Box<LlmConfig>),
    WebSearch(Box<WebSearchToolConfig>),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum ConfigResponse {
    Llm {
        config: Box<LlmConfig>,
        model_readiness: ModelReadiness,
        provider_keys: BTreeMap<String, ProviderKeyStatus>,
        provider_capabilities: BTreeMap<String, ProviderProtocolCapabilities>,
    },
    WebSearch {
        config: Box<WebSearchToolConfig>,
    },
}

impl From<talos_config::LlmConfigRead> for ConfigResponse {
    fn from(value: talos_config::LlmConfigRead) -> Self {
        let provider_capabilities = value.config.protocol_capabilities();
        Self::Llm {
            config: Box::new(value.config),
            model_readiness: value.model_readiness,
            provider_keys: value.provider_keys,
            provider_capabilities,
        }
    }
}

impl From<WebSearchToolConfig> for ConfigResponse {
    fn from(config: WebSearchToolConfig) -> Self {
        Self::WebSearch {
            config: Box::new(config),
        }
    }
}
