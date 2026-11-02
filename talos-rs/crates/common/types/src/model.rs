use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use talos_config::{
    is_managed_request_parameter, CustomProviderConfig, LlmConfig, LlmProtocol, ModelCostConfig,
    ModelInputKind, ModelRef, ModelSelectionCapabilities, ModelSpeed, ProviderConfig,
    ProviderHttpTimeouts, ProviderModelConfig, ReasoningControl, ReasoningEffort, ReasoningReplay,
};

pub type TalosConfig = talos_config::Config;

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ModelListRequest {
    /// Omitted or null preserves the existing target selection.
    #[serde(default, rename = "nodeId")]
    #[ts(rename = "nodeId", optional)]
    pub node_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelListResponse {
    pub models: Vec<ModelInfo>,
    pub default_model_selection: ModelSelection,
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
        let capabilities = model.selection_capabilities();
        Self {
            model_ref,
            reasoning_effort: capabilities.default_reasoning_effort,
        }
    }

    /// Resolves the product-level selection against the selected model's capabilities.
    pub fn resolve_reasoning(
        &self,
        model: &ModelInfo,
    ) -> crate::TalosResult<Option<ReasoningEffort>> {
        if self.model_ref.provider != model.provider || self.model_ref.id != model.model.id {
            return Err(crate::TalosError::new(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "model selection '{}:{}' does not match model metadata '{}:{}'",
                    self.model_ref.provider, self.model_ref.id, model.provider, model.model.id
                ),
            ));
        }

        model
            .model
            .validate_reasoning_effort(self.reasoning_effort)?;
        Ok(self.reasoning_effort)
    }
}

/// Adds provider identity to canonical configured model metadata.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct ModelInfo {
    pub provider: String,
    /// Display label only; routing continues to use provider.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub provider_name: Option<String>,
    /// Configuration-derived controls for clients; never a second capability store.
    #[serde(default)]
    pub selection_capabilities: ModelSelectionCapabilities,
    #[serde(flatten)]
    pub model: ProviderModelConfig,
}

impl ModelInfo {
    pub fn from_provider_model(provider: impl Into<String>, model: ProviderModelConfig) -> Self {
        Self {
            provider: provider.into(),
            provider_name: None,
            selection_capabilities: model.selection_capabilities(),
            model,
        }
    }
}

/// Builds the configured model catalog in stable provider and configuration order.
pub fn resolve_model_catalog(llm: &LlmConfig) -> Vec<ModelInfo> {
    llm.providers()
        .into_iter()
        .flat_map(|(provider, config)| {
            config
                .models
                .iter()
                .filter(|model| !model.id.trim().is_empty())
                .cloned()
                .map(move |model| {
                    let mut info = ModelInfo::from_provider_model(provider, model);
                    info.provider_name = llm
                        .custom_providers
                        .get(provider)
                        .map(|entry| entry.name.clone());
                    info
                })
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
            crate::TalosError::new(
                talos_error::ErrorCode::ProviderUnavailable,
                format!("{}:{}", model_ref.provider, model_ref.id),
            )
        })?;
    Ok(ModelSelection::from_model(model_ref, &model.model))
}
