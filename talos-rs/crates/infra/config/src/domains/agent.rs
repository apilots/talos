use serde::{Deserialize, Serialize};
use talos_error::TalosResult;

use crate::AgentKernelConfig;

fn default_agent_id() -> String {
    "default".to_string()
}

fn default_agent_enabled() -> bool {
    true
}

fn default_max_messages_per_session() -> u32 {
    10
}

/// Bounded in-memory message queue settings for each session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MessageQueueConfig {
    #[serde(default = "default_max_messages_per_session")]
    pub max_messages_per_session: u32,
}

impl Default for MessageQueueConfig {
    fn default() -> Self {
        Self {
            max_messages_per_session: default_max_messages_per_session(),
        }
    }
}

/// Runtime family selected by an Agent definition.
///
/// A new kind must have a matching AgentRuntimeConfig variant and Harness
/// factory support. Unsupported kinds must not silently fall back to Kernel.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    #[default]
    Kernel,
    Codex,
    Claude,
}

/// Typed runtime configuration for an Agent definition.
///
/// This is configuration schema only. Runtime-private clients, handles, or
/// continuation references must not be stored here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentRuntimeConfig {
    Kernel {
        #[serde(default)]
        kernel: AgentKernelConfig,
    },
}

impl Default for AgentRuntimeConfig {
    fn default() -> Self {
        Self::Kernel {
            kernel: AgentKernelConfig::default(),
        }
    }
}

/// Config-file representation of an Agent definition.
///
/// The config layer owns parsing and schema shape. The Harness layer converts
/// this into an AgentDefinition domain object before building a runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDefinitionConfig {
    /// Stable Agent id used by Coordinator routing.
    #[serde(default = "default_agent_id")]
    pub id: String,

    /// Human-readable Agent name.
    #[serde(default = "default_agent_name")]
    pub name: String,

    /// Runtime family for this Agent.
    #[serde(default)]
    pub kind: AgentKind,

    /// Whether this definition should be instantiated.
    #[serde(default = "default_agent_enabled")]
    pub enabled: bool,

    /// Optional human-readable description.
    #[serde(default)]
    pub description: Option<String>,

    /// Default execution workspace key, not a filesystem path.
    #[serde(default)]
    pub default_workspace_id: Option<String>,

    /// Optional runtime-specific configuration matched with kind.
    #[serde(default)]
    pub runtime_config: Option<AgentRuntimeConfig>,
}

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

    /// Persona file
    #[serde(default)]
    pub identity: Option<String>,

    /// Session-scoped in-memory message queue settings.
    #[serde(default)]
    pub message_queue: MessageQueueConfig,

    /// Agent kernel runtime settings
    #[serde(default)]
    pub kernel: AgentKernelConfig,

    /// Optional multi-agent definitions. Empty means the default agent is synthesized.
    #[serde(default)]
    pub agents: Vec<AgentDefinitionConfig>,
}

fn default_agent_name() -> String {
    "talos".to_string()
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            name: default_agent_name(),
            workspace: None,
            identity: None,
            message_queue: MessageQueueConfig::default(),
            kernel: AgentKernelConfig::default(),
            agents: Vec::new(),
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
            other => Err(talos_error::TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("invalid planner mode: {}", other),
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::BuiltIn => "built_in",
        }
    }
}
