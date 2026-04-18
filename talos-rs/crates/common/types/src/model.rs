use serde::{Deserialize, Serialize};

pub use talos_config::{ModelCompatConfig, ModelCostConfig, ModelInputKind, ToolApprovalPolicy};

pub type TalosConfig = talos_config::Config;

/// Public model metadata exposed by the gateway.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub provider: String,
    #[serde(default)]
    pub context_window: u32,
    #[serde(default)]
    pub max_tokens: u32,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub input: Vec<ModelInputKind>,
    #[serde(default)]
    pub cost: ModelCostConfig,
    #[serde(default)]
    pub compat: ModelCompatConfig,
}
