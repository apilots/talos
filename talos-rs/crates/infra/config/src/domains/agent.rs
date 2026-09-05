use serde::{Deserialize, Serialize};
use talos_error::TalosResult;

use crate::{AgentKernelConfig, ModelRef};

fn default_agent_id() -> String {
    "default".to_string()
}

fn default_agent_enabled() -> bool {
    true
}

fn default_max_messages_per_session() -> u32 {
    10
}

fn default_timeline_replay_batch_size() -> usize {
    1_024
}

fn default_command_idempotency_ttl_hours() -> u32 {
    24
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

/// Typed Provider configuration for an Agent definition.
///
/// Add a variant only with a working AgentProvider implementation and
/// composition-root branch. Provider-private clients, handles, or
/// continuation references must not be stored here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentProviderConfig {
    Kernel {
        #[serde(default)]
        kernel: AgentKernelConfig,
    },
}

impl Default for AgentProviderConfig {
    fn default() -> Self {
        Self::Kernel {
            kernel: AgentKernelConfig::default(),
        }
    }
}

/// Config-file representation of an Agent definition.
///
/// The config layer owns parsing and schema shape. The Harness layer converts
/// this into an AgentDefinition domain object before building a Provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDefinitionConfig {
    /// Stable Agent id used by Coordinator routing.
    #[serde(default = "default_agent_id")]
    pub id: String,

    /// Human-readable Agent name.
    #[serde(default = "default_agent_name")]
    pub name: String,

    /// Whether this definition should be instantiated.
    #[serde(default = "default_agent_enabled")]
    pub enabled: bool,

    /// Optional human-readable description.
    #[serde(default)]
    pub description: Option<String>,

    /// Optional typed Provider configuration; omitted uses the default Kernel Provider.
    #[serde(default)]
    pub provider: Option<AgentProviderConfig>,
}

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    /// Agent name
    #[serde(default = "default_agent_name")]
    pub name: String,

    /// Persona file
    #[serde(default)]
    pub identity: Option<String>,

    /// Optional model used by Agent-owned auxiliary model calls.
    #[serde(default)]
    pub inner_model: Option<ModelRef>,

    /// Session-scoped in-memory message queue settings.
    #[serde(default)]
    pub message_queue: MessageQueueConfig,

    /// Maximum canonical events fetched for one replay page.
    #[serde(default = "default_timeline_replay_batch_size")]
    pub timeline_replay_batch_size: usize,

    /// Retention period for completed command idempotency facts.
    #[serde(default = "default_command_idempotency_ttl_hours")]
    pub command_idempotency_ttl_hours: u32,

    /// Default Kernel Provider settings.
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
            identity: None,
            inner_model: None,
            message_queue: MessageQueueConfig::default(),
            timeline_replay_batch_size: default_timeline_replay_batch_size(),
            command_idempotency_ttl_hours: default_command_idempotency_ttl_hours(),
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
