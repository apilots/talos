//! Named connections select a wire protocol and canonical model configuration.
use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_error::{ErrorCode, TalosError, TalosResult};
use ts_rs::TS;

use super::network::{
    allowed_llm_protocols, provider_protocol_capabilities, LlmConfig, LlmProtocol, ProviderConfig,
    ProviderProtocolCapabilities,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct CustomProviderConfig {
    pub name: String,
    #[serde(flatten)]
    pub config: ProviderConfig,
}

impl LlmConfig {
    /// Built-ins retain their order; custom IDs follow in stable sorted order.
    pub fn providers(&self) -> Vec<(&str, &ProviderConfig)> {
        let mut providers = vec![
            ("anthropic", &self.anthropic),
            ("openai", &self.openai),
            ("bailian", &self.bailian),
            ("deepseek", &self.deepseek),
            ("ollama", &self.ollama),
            ("openrouter", &self.openrouter),
        ];
        providers.extend(
            self.custom_providers
                .iter()
                .map(|(id, entry)| (id.as_str(), &entry.config)),
        );
        providers
    }

    pub fn protocol_capabilities(&self) -> BTreeMap<String, ProviderProtocolCapabilities> {
        let mut capabilities = provider_protocol_capabilities();
        for (id, entry) in &self.custom_providers {
            capabilities.insert(
                id.clone(),
                ProviderProtocolCapabilities {
                    default_protocol: entry.config.protocol.unwrap_or(LlmProtocol::Chat),
                    protocols: vec![
                        LlmProtocol::Chat,
                        LlmProtocol::Messages,
                        LlmProtocol::Responses,
                    ],
                },
            );
        }
        capabilities
    }

    /// Validates user-owned instance metadata and provider request extensions.
    pub fn validate_providers(&self) -> TalosResult<()> {
        for (id, entry) in &self.custom_providers {
            let valid_id = id.len() <= 64
                && id.starts_with(|c: char| c.is_ascii_lowercase())
                && id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if !valid_id || allowed_llm_protocols(id).is_some() {
                return Err(invalid("Custom provider ID must be unique, lowercase, and use letters, digits or hyphens"));
            }
            if entry.config.protocol.is_none() {
                return Err(invalid("Custom provider protocol is required"));
            }
            if entry.name.trim().is_empty() {
                return Err(invalid("Custom provider name is required"));
            }
            if entry
                .config
                .api_key
                .as_deref()
                .is_some_and(|key| key.chars().any(char::is_control))
            {
                return Err(invalid("API key contains invalid HTTP header characters"));
            }
            let url = entry.config.base_url.as_deref().unwrap_or_default();
            let parsed = url::Url::parse(url)
                .map_err(|_| invalid("Custom provider Base URL must be HTTP(S)"))?;
            if !matches!(parsed.scheme(), "http" | "https")
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(invalid("Custom provider Base URL must be HTTP(S) without credentials, query or fragment"));
            }
        }
        for (id, provider) in self.providers() {
            if let Some(protocol) = provider.protocol {
                if !self.custom_providers.contains_key(id)
                    && !allowed_llm_protocols(id).is_some_and(|allowed| allowed.contains(&protocol))
                {
                    return Err(invalid(
                        "The selected provider does not support this protocol",
                    ));
                }
            }
            let mut ids = std::collections::BTreeSet::new();
            for model in &provider.models {
                model.validate_selection_controls()?;
                if model.reasoning_replay != super::network::ReasoningReplay::ProtocolDefault
                    && (!self.custom_providers.contains_key(id)
                        || provider.protocol != Some(LlmProtocol::Chat))
                {
                    return Err(invalid(
                        "Reasoning replay overrides require a custom Chat provider",
                    ));
                }
                if model.id.trim().is_empty() || !ids.insert(&model.id) {
                    return Err(invalid(
                        "Model IDs must be nonempty and unique within a provider",
                    ));
                }
                for key in model.request_parameters.keys() {
                    if key.trim().is_empty()
                        || key.trim() != key
                        || is_managed_request_parameter(key)
                    {
                        return Err(invalid(
                            "Request parameter name is empty or reserved by the runtime",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Request extensions cannot replace the runtime's conversation or transport envelope.
pub fn is_managed_request_parameter(key: &str) -> bool {
    matches!(
        key,
        "reasoning_replay"
            | "model"
            | "messages"
            | "input"
            | "system"
            | "tools"
            | "tool_choice"
            | "parallel_tool_calls"
            | "stream"
            | "stream_options"
            | "api_key"
            | "authorization"
    )
}

pub fn custom_provider_key_env(id: &str) -> String {
    format!(
        "TALOS_LLM_CUSTOM_{}_API_KEY",
        id.replace('-', "_").to_ascii_uppercase()
    )
}

fn invalid(message: &str) -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, message)
}

#[cfg(test)]
#[path = "custom_provider_tests.rs"]
mod tests;
