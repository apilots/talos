use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use talos_config::{
    LlmConfig, LlmProtocol, ModelCostConfig, ModelInputKind, ModelSpeed, ProviderConfig,
    ProviderModelConfig, ReasoningEffort,
};

pub type TalosConfig = talos_config::Config;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ModelListRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelListResponse {
    pub models: Vec<ModelInfo>,
    pub default_model_selection: ModelSelection,
}

/// Identifies a concrete model under a provider for request-time selection.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ModelRef {
    pub provider: String,
    pub id: String,
}

/// Selects a model and the reasoning policy applied to requests using it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelSelection {
    pub model_ref: ModelRef,
    pub reasoning_effort: Option<ReasoningEffort>,
}

impl ModelSelection {
    pub fn new(model_ref: ModelRef, reasoning_effort: Option<ReasoningEffort>) -> Self {
        Self {
            model_ref,
            reasoning_effort,
        }
    }

    pub fn without_reasoning(model_ref: ModelRef) -> Self {
        Self::new(model_ref, None)
    }

    /// Creates the explicit default selection declared by configured model metadata.
    pub fn from_model(model_ref: ModelRef, model: &ProviderModelConfig) -> Self {
        Self {
            model_ref,
            reasoning_effort: model.reasoning.then_some(model.reasoning_effort),
        }
    }

    /// Resolves the product-level selection against the selected model's capabilities.
    pub fn resolve_reasoning(&self, model: &ModelInfo) -> crate::TalosResult<EffectiveReasoning> {
        if self.model_ref.provider != model.provider || self.model_ref.id != model.model.id {
            return Err(crate::TalosError::ValidationError(format!(
                "model selection '{}:{}' does not match model metadata '{}:{}'",
                self.model_ref.provider, self.model_ref.id, model.provider, model.model.id
            )));
        }

        match (model.model.reasoning, self.reasoning_effort) {
            (false, Some(_)) => Err(crate::TalosError::ValidationError(format!(
                "model '{}:{}' does not support reasoning effort selection",
                model.provider, model.model.id
            ))),
            (_, None) => Ok(EffectiveReasoning::Unsupported),
            (true, Some(level)) => Ok(EffectiveReasoning::Effort(level)),
        }
    }
}

/// Reasoning policy resolved from a model's capabilities and a request selection.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "type", content = "level", rename_all = "camelCase")]
pub enum EffectiveReasoning {
    #[default]
    Unsupported,
    Effort(ReasoningEffort),
}

/// Adds provider identity to canonical configured model metadata.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct ModelInfo {
    pub provider: String,
    #[serde(flatten)]
    pub model: ProviderModelConfig,
}

impl ModelInfo {
    pub fn from_provider_model(provider: impl Into<String>, model: ProviderModelConfig) -> Self {
        Self {
            provider: provider.into(),
            model,
        }
    }
}

/// Builds the configured model catalog in stable provider and configuration order.
pub fn resolve_model_catalog(llm: &LlmConfig) -> Vec<ModelInfo> {
    [
        ("anthropic", &llm.anthropic),
        ("openai", &llm.openai),
        ("bailian", &llm.bailian),
        ("deepseek", &llm.deepseek),
        ("ollama", &llm.ollama),
        ("openrouter", &llm.openrouter),
    ]
    .into_iter()
    .flat_map(|(provider, config)| {
        config
            .models
            .iter()
            .filter(|model| !model.id.trim().is_empty())
            .cloned()
            .map(move |model| ModelInfo::from_provider_model(provider, model))
    })
    .collect()
}

/// Resolves the configured default model into a complete persisted selection.
pub fn resolve_default_model_selection(llm: &LlmConfig) -> crate::TalosResult<ModelSelection> {
    let model_ref = ModelRef {
        provider: llm.provider.clone(),
        id: llm.model.clone(),
    };
    let model = resolve_model_catalog(llm)
        .into_iter()
        .find(|model| model.provider == model_ref.provider && model.model.id == model_ref.id)
        .ok_or_else(|| {
            crate::TalosError::UnsupportedModel(format!("{}:{}", model_ref.provider, model_ref.id))
        })?;
    Ok(ModelSelection::from_model(model_ref, &model.model))
}
