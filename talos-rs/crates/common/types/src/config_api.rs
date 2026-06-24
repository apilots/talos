use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::LlmConfig;

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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ModelReadiness {
    pub ready: bool,
    pub provider: String,
    pub model: String,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ProviderKeyStatus {
    pub api_key_configured: bool,
}

impl From<talos_config::LlmConfigRead> for ConfigGetResponse {
    fn from(value: talos_config::LlmConfigRead) -> Self {
        Self {
            config: value.config.into(),
            model_readiness: value.model_readiness.into(),
            provider_keys: value
                .provider_keys
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect(),
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
        }
    }
}

impl From<talos_config::ModelReadiness> for ModelReadiness {
    fn from(value: talos_config::ModelReadiness) -> Self {
        Self {
            ready: value.ready,
            provider: value.provider,
            model: value.model,
            issues: value.issues,
        }
    }
}

impl From<talos_config::ProviderKeyStatus> for ProviderKeyStatus {
    fn from(value: talos_config::ProviderKeyStatus) -> Self {
        Self {
            api_key_configured: value.api_key_configured,
        }
    }
}
