//! Lossless projections of existing configuration DTOs; no configuration mirror.
use super::environment_pb::invalid;
use crate::harness_pb::{decode_model, encode_model};
use agent_pb as pb;
use talos_config::{
    CustomProviderConfig, LlmConfig, LlmProtocol, ProviderConfig, WebSearchProviderConfig,
    WebSearchProviderRuntimeConfig, WebSearchToolConfig,
};
use talos_error::TalosResult;
use talos_types::*;
fn protocol(value: LlmProtocol) -> i32 {
    match value {
        LlmProtocol::Chat => 1,
        LlmProtocol::Responses => 2,
        LlmProtocol::Messages => 3,
    }
}
fn decode_protocol(value: i32) -> TalosResult<LlmProtocol> {
    match value {
        1 => Ok(LlmProtocol::Chat),
        2 => Ok(LlmProtocol::Responses),
        3 => Ok(LlmProtocol::Messages),
        _ => Err(invalid()),
    }
}
fn encode_provider(name: &str, value: ProviderConfig) -> pb::ProviderConfig {
    pb::ProviderConfig {
        http_timeouts: value.http_timeouts.map(|value| pb::ProviderHttpTimeouts {
            connect_secs: value.connect_secs,
            response_secs: value.response_secs,
            stream_idle_secs: value.stream_idle_secs,
            stream_total_secs: value.stream_total_secs,
        }),
        api_key: value.api_key,
        base_url: value.base_url,
        protocol: value.protocol.map(protocol),
        models: value
            .models
            .into_iter()
            .map(|model| encode_model(ModelInfo::from_provider_model(name.to_owned(), model)))
            .collect(),
    }
}
fn decode_provider(value: Option<pb::ProviderConfig>) -> TalosResult<ProviderConfig> {
    let value = value.ok_or_else(invalid)?;
    Ok(ProviderConfig {
        http_timeouts: value
            .http_timeouts
            .map(|value| -> TalosResult<_> {
                let timeouts = ProviderHttpTimeouts {
                    connect_secs: value.connect_secs,
                    response_secs: value.response_secs,
                    stream_idle_secs: value.stream_idle_secs,
                    stream_total_secs: value.stream_total_secs,
                };
                timeouts.validate()?;
                Ok(timeouts)
            })
            .transpose()?,
        api_key: value.api_key,
        base_url: value.base_url,
        protocol: value.protocol.map(decode_protocol).transpose()?,
        models: value
            .models
            .into_iter()
            .map(|model| Ok(decode_model(Some(model))?.model))
            .collect::<TalosResult<_>>()?,
    })
}
fn encode_llm(value: LlmConfig) -> pb::LlmConfig {
    pb::LlmConfig {
        provider: value.provider,
        model: value.model,
        custom_providers: value
            .custom_providers
            .into_iter()
            .map(|(id, entry)| {
                let config = encode_provider(&id, entry.config);
                (
                    id,
                    pb::CustomProviderConfig {
                        name: entry.name,
                        config: Some(config),
                    },
                )
            })
            .collect(),
        anthropic: Some(encode_provider("anthropic", value.anthropic)),
        openai: Some(encode_provider("openai", value.openai)),
        bailian: Some(encode_provider("bailian", value.bailian)),
        deepseek: Some(encode_provider("deepseek", value.deepseek)),
        ollama: Some(encode_provider("ollama", value.ollama)),
        openrouter: Some(encode_provider("openrouter", value.openrouter)),
    }
}
fn decode_llm(value: pb::LlmConfig) -> TalosResult<LlmConfig> {
    Ok(LlmConfig {
        custom_providers: value
            .custom_providers
            .into_iter()
            .map(|(id, entry)| {
                Ok((
                    id,
                    CustomProviderConfig {
                        name: entry.name,
                        config: decode_provider(entry.config)?,
                    },
                ))
            })
            .collect::<TalosResult<_>>()?,
        provider: value.provider,
        model: value.model,
        anthropic: decode_provider(value.anthropic)?,
        openai: decode_provider(value.openai)?,
        bailian: decode_provider(value.bailian)?,
        deepseek: decode_provider(value.deepseek)?,
        ollama: decode_provider(value.ollama)?,
        openrouter: decode_provider(value.openrouter)?,
    })
}
fn encode_search_provider(value: WebSearchProviderRuntimeConfig) -> pb::WebSearchProviderConfig {
    pb::WebSearchProviderConfig {
        api_key: value.api_key,
        base_url: value.base_url,
        timeout_ms: value.timeout_ms,
    }
}
fn decode_search_provider(
    value: Option<pb::WebSearchProviderConfig>,
) -> TalosResult<WebSearchProviderRuntimeConfig> {
    let value = value.ok_or_else(invalid)?;
    Ok(WebSearchProviderRuntimeConfig {
        api_key: value.api_key,
        base_url: value.base_url,
        timeout_ms: value.timeout_ms,
    })
}
fn encode_search(value: WebSearchToolConfig) -> pb::WebSearchConfig {
    pb::WebSearchConfig {
        provider: match value.provider {
            WebSearchProviderConfig::Firecrawl => 1,
            WebSearchProviderConfig::Brave => 2,
            WebSearchProviderConfig::Tavily => 3,
        },
        firecrawl: Some(encode_search_provider(value.firecrawl)),
        brave: Some(encode_search_provider(value.brave)),
        tavily: Some(encode_search_provider(value.tavily)),
    }
}
fn decode_search(value: pb::WebSearchConfig) -> TalosResult<WebSearchToolConfig> {
    Ok(WebSearchToolConfig {
        provider: match value.provider {
            1 => WebSearchProviderConfig::Firecrawl,
            2 => WebSearchProviderConfig::Brave,
            3 => WebSearchProviderConfig::Tavily,
            _ => return Err(invalid()),
        },
        firecrawl: decode_search_provider(value.firecrawl)?,
        brave: decode_search_provider(value.brave)?,
        tavily: decode_search_provider(value.tavily)?,
    })
}
pub fn encode_get_config_params(value: ConfigGetRequest) -> TalosResult<pb::ConfigGetParams> {
    Ok(pb::ConfigGetParams {
        path: match value.path {
            ConfigPath::Llm => 1,
            ConfigPath::WebSearch => 2,
        },
    })
}
pub fn decode_get_config_params(
    value: Option<pb::ConfigGetParams>,
) -> TalosResult<ConfigGetRequest> {
    let value = value.ok_or_else(invalid)?;
    Ok(ConfigGetRequest {
        node_id: None,
        path: match value.path {
            1 => ConfigPath::Llm,
            2 => ConfigPath::WebSearch,
            _ => return Err(invalid()),
        },
    })
}
pub fn encode_update_config_params(
    value: ConfigUpdateRequest,
) -> TalosResult<pb::ConfigUpdateParams> {
    Ok(pb::ConfigUpdateParams {
        value: Some(match value.value {
            ConfigValue::Llm(value) => pb::config_update_params::Value::Llm(encode_llm(*value)),
            ConfigValue::WebSearch(value) => {
                pb::config_update_params::Value::WebSearch(encode_search(*value))
            }
        }),
    })
}
pub fn decode_update_config_params(
    value: Option<pb::ConfigUpdateParams>,
) -> TalosResult<ConfigUpdateRequest> {
    Ok(ConfigUpdateRequest {
        node_id: None,
        value: match value.ok_or_else(invalid)?.value.ok_or_else(invalid)? {
            pb::config_update_params::Value::Llm(value) => {
                ConfigValue::Llm(Box::new(decode_llm(value)?))
            }
            pb::config_update_params::Value::WebSearch(value) => {
                ConfigValue::WebSearch(Box::new(decode_search(value)?))
            }
        },
    })
}
pub fn encode_delete_config_params(
    value: ConfigDeleteRequest,
) -> TalosResult<pb::ConfigDeleteParams> {
    Ok(pb::ConfigDeleteParams { path: value.path })
}
pub fn decode_delete_config_params(
    value: Option<pb::ConfigDeleteParams>,
) -> TalosResult<ConfigDeleteRequest> {
    Ok(ConfigDeleteRequest {
        node_id: None,
        path: value.ok_or_else(invalid)?.path,
    })
}
pub fn encode_reload_config_params(_: ConfigReloadRequest) -> TalosResult<pb::Empty> {
    Ok(pb::Empty {})
}
pub fn decode_reload_config_params(value: Option<pb::Empty>) -> TalosResult<ConfigReloadRequest> {
    value.ok_or_else(invalid)?;
    Ok(ConfigReloadRequest::default())
}
pub fn encode_config_result(value: ConfigResponse) -> TalosResult<pb::ConfigRead> {
    Ok(pb::ConfigRead {
        value: Some(match value {
            ConfigResponse::Llm {
                config,
                model_readiness,
                provider_keys,
                provider_capabilities,
            } => pb::config_read::Value::Llm(pb::LlmConfigRead {
                config: Some(encode_llm(*config)),
                model_readiness: Some(pb::ModelReadiness {
                    ready: model_readiness.ready,
                    provider: model_readiness.provider,
                    model: model_readiness.model,
                    issues: model_readiness.issues,
                }),
                provider_keys: provider_keys
                    .into_iter()
                    .map(|(key, value)| {
                        (
                            key,
                            pb::ProviderKeyStatus {
                                api_key_configured: value.api_key_configured,
                            },
                        )
                    })
                    .collect(),
                provider_capabilities: provider_capabilities
                    .into_iter()
                    .map(|(key, value)| {
                        (
                            key,
                            pb::ProviderProtocolCapabilities {
                                default_protocol: protocol(value.default_protocol),
                                protocols: value.protocols.into_iter().map(protocol).collect(),
                            },
                        )
                    })
                    .collect(),
            }),
            ConfigResponse::WebSearch { config } => {
                pb::config_read::Value::WebSearch(encode_search(*config))
            }
        }),
    })
}
pub fn decode_config_result(value: pb::ConfigRead) -> TalosResult<ConfigResponse> {
    Ok(match value.value.ok_or_else(invalid)? {
        pb::config_read::Value::Llm(value) => {
            let readiness = value.model_readiness.ok_or_else(invalid)?;
            ConfigResponse::Llm {
                config: Box::new(decode_llm(value.config.ok_or_else(invalid)?)?),
                model_readiness: ModelReadiness {
                    ready: readiness.ready,
                    provider: readiness.provider,
                    model: readiness.model,
                    issues: readiness.issues,
                },
                provider_keys: value
                    .provider_keys
                    .into_iter()
                    .map(|(key, value)| {
                        (
                            key,
                            ProviderKeyStatus {
                                api_key_configured: value.api_key_configured,
                            },
                        )
                    })
                    .collect(),
                provider_capabilities: value
                    .provider_capabilities
                    .into_iter()
                    .map(|(key, value)| {
                        Ok((
                            key,
                            ProviderProtocolCapabilities {
                                default_protocol: decode_protocol(value.default_protocol)?,
                                protocols: value
                                    .protocols
                                    .into_iter()
                                    .map(decode_protocol)
                                    .collect::<TalosResult<_>>()?,
                            },
                        ))
                    })
                    .collect::<TalosResult<_>>()?,
            }
        }
        pb::config_read::Value::WebSearch(value) => ConfigResponse::WebSearch {
            config: Box::new(decode_search(value)?),
        },
    })
}
pub fn encode_delete_config_result(value: ConfigDeleteResponse) -> TalosResult<pb::ConfigDeleted> {
    Ok(pb::ConfigDeleted {
        success: value.success,
    })
}
pub fn decode_delete_config_result(value: pb::ConfigDeleted) -> TalosResult<ConfigDeleteResponse> {
    Ok(ConfigDeleteResponse {
        success: value.success,
    })
}
pub fn encode_reload_config_result(value: ConfigReloadResponse) -> TalosResult<pb::ConfigReloaded> {
    Ok(pb::ConfigReloaded {
        success: value.success,
    })
}
pub fn decode_reload_config_result(value: pb::ConfigReloaded) -> TalosResult<ConfigReloadResponse> {
    Ok(ConfigReloadResponse {
        success: value.success,
    })
}

#[cfg(test)]
#[path = "environment_config_pb_tests.rs"]
mod tests;
