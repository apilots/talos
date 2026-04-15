use serde::{Deserialize, Serialize};
use std::fmt;
use talos_error::TalosResult;

use crate::AgentKernelConfig;

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    PlanReAct,
}

impl PlannerMode {
    pub fn parse(value: &str) -> TalosResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "built_in" => Ok(Self::BuiltIn),
            "plan_react" => Ok(Self::PlanReAct),
            other => Err(talos_error::TalosError::ValidationError(format!(
                "invalid planner mode: {}",
                other
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::BuiltIn => "built_in",
            Self::PlanReAct => "plan_react",
        }
    }
}

/// Controls how the planner is selected for each turn.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlannerSelection {
    /// Always use the configured `planner_mode` value. No LLM inference.
    #[default]
    Static,
    /// Use an LLM classifier to select the planner each turn.
    /// Falls back to `planner_mode` on classifier failure.
    Auto,
}

impl fmt::Display for PlannerMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Loop control policy for PlanReAct mode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanReActPolicy {
    /// Maximum number of replanning iterations allowed per turn.
    #[serde(default = "default_plan_react_max_replans")]
    pub max_replans: u32,
    /// Maximum consecutive no-progress rounds before aborting.
    #[serde(default = "default_plan_react_max_no_progress_rounds")]
    pub max_no_progress_rounds: u32,
    /// Whether the model must output a FINAL_ANSWER tag to end the turn.
    #[serde(default = "default_true")]
    pub require_final_answer_tag: bool,
}

impl Default for PlanReActPolicy {
    fn default() -> Self {
        Self {
            max_replans: default_plan_react_max_replans(),
            max_no_progress_rounds: default_plan_react_max_no_progress_rounds(),
            require_final_answer_tag: true,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_plan_react_max_replans() -> u32 {
    3
}

fn default_plan_react_max_no_progress_rounds() -> u32 {
    2
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    #[default]
    Single,
    Sequential,
    Parallel,
    Loop,
}

impl ExecutionMode {
    pub fn parse(value: &str) -> TalosResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "single" => Ok(Self::Single),
            "sequential" => Ok(Self::Sequential),
            "parallel" => Ok(Self::Parallel),
            "loop" => Ok(Self::Loop),
            other => Err(talos_error::TalosError::ValidationError(format!(
                "invalid execution mode: {}",
                other
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Sequential => "sequential",
            Self::Parallel => "parallel",
            Self::Loop => "loop",
        }
    }
}

impl fmt::Display for ExecutionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
