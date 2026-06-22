use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub type TalosConfig = talos_config::Config;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ModelListRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct ModelListResponse {
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    pub anthropic: LlmProviderConfig,
    pub openai: LlmProviderConfig,
    pub bailian: LlmProviderConfig,
    pub ollama: LlmProviderConfig,
    pub openrouter: LlmProviderConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct LlmProviderConfig {
    #[serde(default)]
    #[ts(optional)]
    pub api_key: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub base_url: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    #[ts(optional)]
    pub protocol: Option<LlmProtocol>,
    #[serde(default)]
    pub models: Vec<LlmModelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct LlmModelConfig {
    pub id: String,
    pub name: String,
    pub context_window: u32,
    pub reasoning: bool,
    pub reasoning_effort: LlmReasoningEffort,
    pub speed: LlmModelSpeed,
    pub input: Vec<LlmModelInputKind>,
    pub cost: LlmModelCost,
    pub compat: LlmModelCompat,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LlmProtocol {
    #[default]
    Chat,
    Responses,
    Messages,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LlmReasoningEffort {
    Low,
    #[default]
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LlmModelSpeed {
    #[default]
    Standard,
    Fast,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmModelInputKind {
    Text,
    Image,
    Audio,
    Video,
    File,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct LlmModelCost {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq, Default)]
pub struct LlmModelCompat {
    #[serde(default)]
    #[ts(optional)]
    pub thinking_format: Option<LlmThinkingFormat>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmThinkingFormat {
    OpenAi,
    Anthropic,
    Qwen,
}

pub type ReasoningEffort = LlmReasoningEffort;
pub type ModelSpeed = LlmModelSpeed;
pub type ModelInputKind = LlmModelInputKind;
pub type ModelCostConfig = LlmModelCost;
pub type ModelCompatConfig = LlmModelCompat;
pub type ThinkingFormat = LlmThinkingFormat;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub context_window: u32,
    pub reasoning: bool,
    pub reasoning_effort: LlmReasoningEffort,
    pub speed: LlmModelSpeed,
    pub input: Vec<LlmModelInputKind>,
    pub cost: LlmModelCost,
    pub compat: LlmModelCompat,
}

impl ModelInfo {
    pub fn from_provider_model(
        provider: impl Into<String>,
        model: talos_config::ProviderModelConfig,
    ) -> Self {
        Self {
            id: model.id.clone(),
            name: model.display_name().to_string(),
            provider: provider.into(),
            context_window: model.context_window,
            reasoning: model.reasoning,
            reasoning_effort: model.reasoning_effort.into(),
            speed: model.speed.into(),
            input: model.input.into_iter().map(Into::into).collect(),
            cost: model.cost.into(),
            compat: model.compat.into(),
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self::from(talos_config::LlmConfig::default())
    }
}

impl From<talos_config::LlmConfig> for LlmConfig {
    fn from(value: talos_config::LlmConfig) -> Self {
        Self {
            provider: value.provider,
            model: value.model,
            anthropic: value.anthropic.into(),
            openai: value.openai.into(),
            bailian: value.bailian.into(),
            ollama: value.ollama.into(),
            openrouter: value.openrouter.into(),
        }
    }
}

impl From<LlmConfig> for talos_config::LlmConfig {
    fn from(value: LlmConfig) -> Self {
        Self {
            provider: value.provider,
            model: value.model,
            anthropic: value.anthropic.into(),
            openai: value.openai.into(),
            bailian: value.bailian.into(),
            ollama: value.ollama.into(),
            openrouter: value.openrouter.into(),
        }
    }
}

impl From<talos_config::ProviderConfig> for LlmProviderConfig {
    fn from(value: talos_config::ProviderConfig) -> Self {
        Self {
            api_key: value.api_key,
            base_url: value.base_url,
            enabled: value.enabled,
            protocol: value.protocol.map(Into::into),
            models: value.models.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<LlmProviderConfig> for talos_config::ProviderConfig {
    fn from(value: LlmProviderConfig) -> Self {
        Self {
            api_key: value.api_key,
            base_url: value.base_url,
            enabled: value.enabled,
            protocol: value.protocol.map(Into::into),
            models: value.models.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<talos_config::ProviderModelConfig> for LlmModelConfig {
    fn from(value: talos_config::ProviderModelConfig) -> Self {
        Self {
            id: value.id,
            name: value.name,
            context_window: value.context_window,
            reasoning: value.reasoning,
            reasoning_effort: value.reasoning_effort.into(),
            speed: value.speed.into(),
            input: value.input.into_iter().map(Into::into).collect(),
            cost: value.cost.into(),
            compat: value.compat.into(),
        }
    }
}

impl From<LlmModelConfig> for talos_config::ProviderModelConfig {
    fn from(value: LlmModelConfig) -> Self {
        Self {
            id: value.id,
            name: value.name,
            context_window: value.context_window,
            reasoning: value.reasoning,
            reasoning_effort: value.reasoning_effort.into(),
            speed: value.speed.into(),
            input: value.input.into_iter().map(Into::into).collect(),
            cost: value.cost.into(),
            compat: value.compat.into(),
        }
    }
}

impl From<talos_config::LlmProtocol> for LlmProtocol {
    fn from(value: talos_config::LlmProtocol) -> Self {
        match value {
            talos_config::LlmProtocol::Chat => Self::Chat,
            talos_config::LlmProtocol::Responses => Self::Responses,
            talos_config::LlmProtocol::Messages => Self::Messages,
        }
    }
}

impl From<LlmProtocol> for talos_config::LlmProtocol {
    fn from(value: LlmProtocol) -> Self {
        match value {
            LlmProtocol::Chat => Self::Chat,
            LlmProtocol::Responses => Self::Responses,
            LlmProtocol::Messages => Self::Messages,
        }
    }
}

impl From<talos_config::ReasoningEffort> for LlmReasoningEffort {
    fn from(value: talos_config::ReasoningEffort) -> Self {
        match value {
            talos_config::ReasoningEffort::Low => Self::Low,
            talos_config::ReasoningEffort::Medium => Self::Medium,
            talos_config::ReasoningEffort::High => Self::High,
            talos_config::ReasoningEffort::XHigh => Self::XHigh,
        }
    }
}

impl From<LlmReasoningEffort> for talos_config::ReasoningEffort {
    fn from(value: LlmReasoningEffort) -> Self {
        match value {
            LlmReasoningEffort::Low => Self::Low,
            LlmReasoningEffort::Medium => Self::Medium,
            LlmReasoningEffort::High => Self::High,
            LlmReasoningEffort::XHigh => Self::XHigh,
        }
    }
}

impl From<talos_config::ModelSpeed> for LlmModelSpeed {
    fn from(value: talos_config::ModelSpeed) -> Self {
        match value {
            talos_config::ModelSpeed::Standard => Self::Standard,
            talos_config::ModelSpeed::Fast => Self::Fast,
        }
    }
}

impl From<LlmModelSpeed> for talos_config::ModelSpeed {
    fn from(value: LlmModelSpeed) -> Self {
        match value {
            LlmModelSpeed::Standard => Self::Standard,
            LlmModelSpeed::Fast => Self::Fast,
        }
    }
}

impl From<talos_config::ModelInputKind> for LlmModelInputKind {
    fn from(value: talos_config::ModelInputKind) -> Self {
        match value {
            talos_config::ModelInputKind::Text => Self::Text,
            talos_config::ModelInputKind::Image => Self::Image,
            talos_config::ModelInputKind::Audio => Self::Audio,
            talos_config::ModelInputKind::Video => Self::Video,
            talos_config::ModelInputKind::File => Self::File,
        }
    }
}

impl From<LlmModelInputKind> for talos_config::ModelInputKind {
    fn from(value: LlmModelInputKind) -> Self {
        match value {
            LlmModelInputKind::Text => Self::Text,
            LlmModelInputKind::Image => Self::Image,
            LlmModelInputKind::Audio => Self::Audio,
            LlmModelInputKind::Video => Self::Video,
            LlmModelInputKind::File => Self::File,
        }
    }
}

impl From<talos_config::ModelCostConfig> for LlmModelCost {
    fn from(value: talos_config::ModelCostConfig) -> Self {
        Self {
            input: value.input,
            output: value.output,
            cache_read: value.cache_read,
            cache_write: value.cache_write,
        }
    }
}

impl From<LlmModelCost> for talos_config::ModelCostConfig {
    fn from(value: LlmModelCost) -> Self {
        Self {
            input: value.input,
            output: value.output,
            cache_read: value.cache_read,
            cache_write: value.cache_write,
        }
    }
}

impl From<talos_config::ModelCompatConfig> for LlmModelCompat {
    fn from(value: talos_config::ModelCompatConfig) -> Self {
        Self {
            thinking_format: value.thinking_format.map(Into::into),
        }
    }
}

impl From<LlmModelCompat> for talos_config::ModelCompatConfig {
    fn from(value: LlmModelCompat) -> Self {
        Self {
            thinking_format: value.thinking_format.map(Into::into),
        }
    }
}

impl From<talos_config::ThinkingFormat> for LlmThinkingFormat {
    fn from(value: talos_config::ThinkingFormat) -> Self {
        match value {
            talos_config::ThinkingFormat::OpenAi => Self::OpenAi,
            talos_config::ThinkingFormat::Anthropic => Self::Anthropic,
            talos_config::ThinkingFormat::Qwen => Self::Qwen,
        }
    }
}

impl From<LlmThinkingFormat> for talos_config::ThinkingFormat {
    fn from(value: LlmThinkingFormat) -> Self {
        match value {
            LlmThinkingFormat::OpenAi => Self::OpenAi,
            LlmThinkingFormat::Anthropic => Self::Anthropic,
            LlmThinkingFormat::Qwen => Self::Qwen,
        }
    }
}

pub fn resolve_model_catalog(llm: &talos_config::LlmConfig) -> Vec<ModelInfo> {
    let provider = llm.provider.clone();
    let provider_models = match llm.provider.as_str() {
        "anthropic" => llm.anthropic.models.clone(),
        "openai" => llm.openai.models.clone(),
        "bailian" => llm.bailian.models.clone(),
        "ollama" => llm.ollama.models.clone(),
        "openrouter" => llm.openrouter.models.clone(),
        _ => Vec::new(),
    };

    let mut models: Vec<ModelInfo> = provider_models
        .into_iter()
        .filter(|model| !model.id.trim().is_empty())
        .map(|model| ModelInfo::from_provider_model(provider.clone(), model))
        .collect();

    if !models.iter().any(|model| model.id == llm.model) {
        models.push(ModelInfo {
            id: llm.model.clone(),
            name: llm.model.clone(),
            provider,
            context_window: 0,
            reasoning: false,
            reasoning_effort: LlmReasoningEffort::default(),
            speed: LlmModelSpeed::default(),
            input: Vec::new(),
            cost: talos_config::ModelCostConfig::default().into(),
            compat: talos_config::ModelCompatConfig::default().into(),
        });
    }

    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    models
}
