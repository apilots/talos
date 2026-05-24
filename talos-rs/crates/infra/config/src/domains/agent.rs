use serde::{Deserialize, Serialize};
use talos_error::TalosResult;

use crate::AgentKernelConfig;

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    /// Agent name
    #[serde(default = "default_agent_name")]
    pub name: String,

    /// Workspace path
    #[serde(default)]
    pub workspace: Option<String>,

    /// Default model
    #[serde(default)]
    pub model: String,

    /// Persona file
    #[serde(default)]
    pub identity: Option<String>,

    /// Agent kernel runtime settings
    #[serde(default)]
    pub kernel: AgentKernelConfig,
}

fn default_agent_name() -> String {
    "talos".to_string()
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: default_agent_name(),
            workspace: None,
            model: String::new(),
            identity: None,
            kernel: AgentKernelConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlannerMode {
    #[default]
    BuiltIn,
}

impl PlannerMode {
    pub fn parse(value: &str) -> TalosResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "built_in" => Ok(Self::BuiltIn),
            other => Err(talos_error::TalosError::ValidationError(format!(
                "invalid planner mode: {}",
                other
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::BuiltIn => "built_in",
        }
    }
}
