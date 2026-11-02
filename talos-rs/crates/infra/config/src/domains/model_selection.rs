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
    pub reasoning_efforts: Vec<ReasoningEffort>,
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
        if let Some(efforts) = &self.reasoning_efforts {
            if self.reasoning && !efforts.is_empty() && !efforts.contains(&self.reasoning_effort) {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Declared reasoning efforts must include the default effort",
                ));
            }
        }
        Ok(())
    }

    /// Canonical validation shared by selection and direct provider callers.
    pub fn validate_reasoning_effort(&self, effort: Option<ReasoningEffort>) -> TalosResult<()> {
        if let Some(effort) = effort {
            let capabilities = self.selection_capabilities();
            let supported = match capabilities.reasoning {
                ReasoningControl::None => false,
                ReasoningControl::Toggle => effort == self.reasoning_effort,
                ReasoningControl::Effort => capabilities.reasoning_efforts.contains(&effort),
            };
            if !supported {
                return Err(TalosError::new(ErrorCode::InvalidRequest, "Selected reasoning effort is not supported by this model; choose an allowed effort"));
            }
        }
        Ok(())
    }

    pub fn selection_capabilities(&self) -> ModelSelectionCapabilities {
        let toggle_default = self.thinking_toggle_values().next();
        let reasoning_efforts = [
            ReasoningEffort::Light,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
            ReasoningEffort::XHigh,
        ]
        .into_iter()
        .filter(|effort| {
            self.reasoning
                && self
                    .reasoning_efforts
                    .as_ref()
                    .is_none_or(|allowed| allowed.contains(effort))
        })
        .collect::<Vec<_>>();
        let reasoning = if !self.reasoning {
            ReasoningControl::None
        } else if !reasoning_efforts.is_empty() {
            ReasoningControl::Effort
        } else if toggle_default.is_some() {
            ReasoningControl::Toggle
        } else {
            ReasoningControl::None
        };
        ModelSelectionCapabilities {
            reasoning,
            reasoning_efforts,
            fast: self.supports_fast_mode(),
            default_fast: self.default_fast_mode(),
            default_reasoning_effort: (reasoning != ReasoningControl::None
                && toggle_default != Some(false))
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
