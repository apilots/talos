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
    pub reasoning: ReasoningSelection,
}

impl ModelSelection {
    /// Creates a selection that follows the selected model's configured reasoning default.
    pub fn model_default(model_ref: ModelRef) -> Self {
        Self {
            model_ref,
            reasoning: ReasoningSelection::ModelDefault,
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

        match (model.model.reasoning, self.reasoning) {
            (false, ReasoningSelection::ModelDefault) => Ok(EffectiveReasoning::Unsupported),
            (false, ReasoningSelection::Effort { .. }) => {
                Err(crate::TalosError::ValidationError(format!(
                    "model '{}:{}' does not support reasoning effort selection",
                    model.provider, model.model.id
                )))
            }
            (true, ReasoningSelection::ModelDefault) => {
                Ok(EffectiveReasoning::Effort(model.model.reasoning_effort))
            }
            (true, ReasoningSelection::Effort { level }) => Ok(EffectiveReasoning::Effort(level)),
        }
    }
}

/// Describes whether a selection follows model defaults or explicitly overrides effort.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ReasoningSelection {
    #[default]
    ModelDefault,
    Effort {
        level: ReasoningEffort,
    },
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
