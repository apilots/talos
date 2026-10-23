use std::sync::Arc;

use talos_config::{effective_llm_protocol, Config, LlmProtocol, ProviderConfig};
use talos_error::{TalosError, TalosResult};
use talos_llm_anthropic::AnthropicProvider;
use talos_llm_bailian::BailianProvider;
use talos_llm_deepseek::{DeepSeekConfig, DeepSeekProtocol, DeepSeekProvider};
use talos_llm_ollama::{OllamaConfig, OllamaProvider};
use talos_llm_openai::{OpenAIProtocol, OpenAIProvider};
use talos_llm_openrouter::{OpenRouterConfig, OpenRouterProtocol, OpenRouterProvider};
use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider};
use tracing::warn;

pub fn create_llm_provider(config: &Config) -> TalosResult<Arc<dyn LlmProvider>> {
    let readiness = config.model_readiness();
    if !readiness.ready {
        warn!(
            provider = readiness.provider,
            model = readiness.model,
            issues = ?readiness.issues,
            "model provider is not ready; runtime will start in setup-required mode"
        );
        return Ok(Arc::new(ModelSetupRequiredProvider {
            model: config.llm.model.clone(),
            message: format!("model setup required: {}", readiness.issues.join("; ")),
        }));
    }

    let selected = config
        .provider_config(&config.llm.provider)
        .ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ProviderUnavailable,
                "Model provider is unavailable",
            )
        })?;
    let inner = if let Some(custom) = config.llm.custom_providers.get(&config.llm.provider) {
        let protocol = custom.config.protocol.ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                "Custom provider protocol is required",
            )
        })?;
        Arc::new(talos_llm_provider::ProtocolProvider::new(
            &config.llm.provider,
            protocol,
            &custom.config,
            &config.llm.model,
        )?) as Arc<dyn LlmProvider>
    } else {
        create_adapter(config)?
    };
    Ok(Arc::new(
        crate::configured_provider::ConfiguredProvider::new(
            config.llm.provider.clone(),
            inner,
            selected,
        ),
    ))
}

fn create_adapter(config: &Config) -> TalosResult<Arc<dyn LlmProvider>> {
    match config.llm.provider.as_str() {
        "anthropic" => {
            let api_key = config.llm.anthropic.api_key.clone().ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "llm.anthropic.api_key is required".to_string(),
                )
            })?;
            let base_url = config
                .llm
                .anthropic
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string());
            let anthropic_config = talos_llm_anthropic::AnthropicConfig::new(&api_key)
                .with_base_url(&base_url)
                .with_default_model(&config.llm.model);
            Ok(Arc::new(AnthropicProvider::new(anthropic_config)))
        }
        "openai" => {
            let api_key = config.llm.openai.api_key.clone().ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "llm.openai.api_key is required".to_string(),
                )
            })?;
            let mut openai_config = talos_llm_openai::OpenAIConfig::new(&api_key)
                .with_default_model(&config.llm.model)
                .with_protocol(openai_protocol("openai", &config.llm.openai)?);
            if let Some(base_url) = config.llm.openai.base_url.as_deref() {
                openai_config = openai_config.with_base_url(base_url);
            }
            Ok(Arc::new(OpenAIProvider::new(openai_config)))
        }
        "bailian" => {
            let api_key = config.llm.bailian.api_key.clone().ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "llm.bailian.api_key is required".to_string(),
                )
            })?;
            let base_url = config.llm.bailian.base_url.clone().ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "llm.bailian.base_url is required".to_string(),
                )
            })?;
            let bailian_config =
                talos_llm_bailian::BailianConfig::new(&api_key).with_base_url(&base_url);
            Ok(Arc::new(BailianProvider::new(bailian_config)))
        }
        "deepseek" => {
            let api_key = config.llm.deepseek.api_key.clone().ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "llm.deepseek.api_key is required".to_string(),
                )
            })?;
            let base_url = config
                .llm
                .deepseek
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.deepseek.com".to_string());
            let deepseek_config = DeepSeekConfig::new(&api_key)
                .with_base_url(&base_url)
                .with_default_model(&config.llm.model)
                .with_protocol(deepseek_protocol(&config.llm.deepseek)?);
            Ok(Arc::new(DeepSeekProvider::new(deepseek_config)))
        }
        "ollama" => {
            let base_url = config.llm.ollama.base_url.clone().ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "llm.ollama.base_url is required".to_string(),
                )
            })?;
            let mut ollama_config = OllamaConfig::new(&base_url, &config.llm.model);
            if let Some(api_key) = config.llm.ollama.api_key.as_deref() {
                ollama_config = ollama_config.with_api_key(api_key);
            }
            Ok(Arc::new(OllamaProvider::new(ollama_config)))
        }
        "openrouter" => {
            let api_key = config.llm.openrouter.api_key.clone().ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::InvalidRequest,
                    "llm.openrouter.api_key is required".to_string(),
                )
            })?;
            let base_url = config
                .llm
                .openrouter
                .base_url
                .clone()
                .unwrap_or_else(|| "https://openrouter.ai/api/v1".to_string());
            let openrouter_config = OpenRouterConfig::new(&api_key)
                .with_base_url(&base_url)
                .with_default_model(&config.llm.model)
                .with_protocol(openrouter_protocol(&config.llm.openrouter)?);
            Ok(Arc::new(OpenRouterProvider::new(openrouter_config)))
        }
        _ => Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("unsupported provider '{}'", config.llm.provider),
        )),
    }
}

fn openai_protocol(
    provider: &str,
    provider_config: &ProviderConfig,
) -> TalosResult<OpenAIProtocol> {
    match effective_llm_protocol(provider, provider_config).ok_or_else(|| {
        TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("llm.{provider}.protocol is required"),
        )
    })? {
        LlmProtocol::Chat => Ok(OpenAIProtocol::Chat),
        LlmProtocol::Responses => Ok(OpenAIProtocol::Responses),
        protocol => Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            format!("llm.{provider}.protocol does not support {protocol}"),
        )),
    }
}

fn openrouter_protocol(provider_config: &ProviderConfig) -> TalosResult<OpenRouterProtocol> {
    match effective_llm_protocol("openrouter", provider_config).ok_or_else(|| {
        TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "llm.openrouter.protocol is required".to_string(),
        )
    })? {
        LlmProtocol::Chat => Ok(OpenRouterProtocol::Chat),
        LlmProtocol::Responses => Ok(OpenRouterProtocol::Responses),
        LlmProtocol::Messages => Ok(OpenRouterProtocol::Messages),
    }
}

fn deepseek_protocol(provider_config: &ProviderConfig) -> TalosResult<DeepSeekProtocol> {
    match effective_llm_protocol("deepseek", provider_config).ok_or_else(|| {
        TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "llm.deepseek.protocol is required".to_string(),
        )
    })? {
        LlmProtocol::Chat => Ok(DeepSeekProtocol::Chat),
        LlmProtocol::Messages => Ok(DeepSeekProtocol::Messages),
        LlmProtocol::Responses => Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "llm.deepseek.protocol does not support responses".to_string(),
        )),
    }
}

struct ModelSetupRequiredProvider {
    model: String,
    message: String,
}

#[async_trait::async_trait]
impl LlmProvider for ModelSetupRequiredProvider {
    fn name(&self) -> &str {
        "setup-required"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            self.message.clone(),
        ))
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            self.message.clone(),
        ))
    }

    fn default_model(&self) -> String {
        self.model.clone()
    }

    async fn stream_model_blocks(
        &self,
        _request: ChatRequest,
    ) -> TalosResult<talos_llm_provider::ModelBlockStream> {
        Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            self.message.clone(),
        ))
    }
}
