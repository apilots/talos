//! Selection controls derived from canonical model configuration.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use talos_error::{ErrorCode, TalosError, TalosResult};
use ts_rs::TS;

use super::network::{ModelSpeed, ProviderModelConfig, ReasoningEffort};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningControl {
    #[default]
    None,
    Toggle,
    Effort,
}

/// Read-only projection; the model's configuration remains the source of truth.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
pub struct ModelSelectionCapabilities {
    pub reasoning: ReasoningControl,
    pub fast: bool,
    pub default_fast: bool,
    pub default_reasoning_effort: Option<ReasoningEffort>,
}

impl ProviderModelConfig {
    fn thinking_toggle_values(&self) -> impl Iterator<Item = bool> {
        let parameters = &self.request_parameters;
        [
            parameters
                .get("enable_thinking")
                .and_then(serde_json::Value::as_bool),
            parameters
                .get("chat_template_kwargs")
                .and_then(|value| value.get("enable_thinking"))
                .and_then(serde_json::Value::as_bool),
            parameters
                .get("thinking")
                .and_then(|value| value.get("type"))
                .and_then(serde_json::Value::as_str)
                .and_then(|value| match value {
                    "enabled" => Some(true),
                    "disabled" => Some(false),
                    _ => None,
                }),
        ]
        .into_iter()
        .flatten()
    }

    pub(crate) fn validate_selection_controls(&self) -> TalosResult<()> {
        let mut values = self.thinking_toggle_values();
        let first = values.next();
        if first.is_some_and(|first| values.any(|value| value != first)) {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Configured thinking toggles must declare the same default",
            ));
        }
        Ok(())
    }

    pub fn selection_capabilities(&self) -> ModelSelectionCapabilities {
        let toggle_default = self.thinking_toggle_values().next();
        let reasoning = if !self.reasoning {
            ReasoningControl::None
        } else if toggle_default.is_some() {
            ReasoningControl::Toggle
        } else {
            ReasoningControl::Effort
        };
        ModelSelectionCapabilities {
            reasoning,
            fast: self.supports_fast_mode(),
            default_fast: self.default_fast_mode(),
            default_reasoning_effort: (self.reasoning && toggle_default != Some(false))
                .then_some(self.reasoning_effort),
        }
    }

    /// Explicit wire configuration advertises metadata only; no session control.
    pub fn fast_service_tier(&self) -> Option<&str> {
        self.request_parameters
            .get("service_tier")
            .and_then(serde_json::Value::as_str)
            .filter(|tier| matches!(*tier, "default" | "fast" | "priority"))
    }

    pub fn supports_fast_mode(&self) -> bool {
        self.fast_service_tier().is_some()
    }

    pub fn default_fast_mode(&self) -> bool {
        self.supports_fast_mode() && self.speed == ModelSpeed::Fast
    }
}

#[cfg(test)]
#[path = "model_selection_tests.rs"]
mod tests;
