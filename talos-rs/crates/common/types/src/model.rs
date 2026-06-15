use serde::{Deserialize, Serialize};

pub use talos_config::{
    LlmConfig, LlmProtocol, ModelCompatConfig, ModelCostConfig, ModelInputKind, ModelSpeed,
    ReasoningEffort,
};

pub type TalosConfig = talos_config::Config;

/// Build the model catalog from an LlmConfig (used at startup and on hot-reload).
pub fn resolve_model_catalog(llm: &LlmConfig) -> Vec<ModelInfo> {
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
        .filter(|m| !m.id.trim().is_empty())
        .map(|m| ModelInfo {
            id: m.id.clone(),
            name: m.display_name().to_string(),
            provider: provider.clone(),
            context_window: m.context_window,
            reasoning: m.reasoning,
            reasoning_effort: m.reasoning_effort,
            speed: m.speed,
            input: m.input.clone(),
            cost: m.cost.clone(),
            compat: m.compat.clone(),
        })
        .collect();

    if !models.iter().any(|m| m.id == llm.model) {
        models.push(ModelInfo {
            id: llm.model.clone(),
            name: llm.model.clone(),
            provider: provider.clone(),
            context_window: 0,
            reasoning: false,
            reasoning_effort: ReasoningEffort::default(),
            speed: ModelSpeed::default(),
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        });
    }

    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    models
}

/// Public model metadata exposed by the gateway.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    #[serde(default)]
    pub context_window: u32,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub reasoning_effort: ReasoningEffort,
    #[serde(default)]
    pub speed: ModelSpeed,
    #[serde(default)]
    pub input: Vec<ModelInputKind>,
    #[serde(default)]
    pub cost: ModelCostConfig,
    #[serde(default)]
    pub compat: ModelCompatConfig,
}
