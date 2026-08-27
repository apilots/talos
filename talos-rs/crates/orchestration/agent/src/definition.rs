use std::collections::HashSet;
use std::sync::Arc;

use talos_agent_kernel::AttachmentReader;
use talos_config::{
    AgentConfig, AgentDefinitionConfig, AgentKind as ConfigAgentKind, AgentRuntimeConfig,
};
use talos_error::{TalosError, TalosResult};
use talos_llm_provider::LlmProviderRouter;
use talos_memory::{MemoryWriter, UserWorkspaceMemoryReader, UserWorkspaceMemoryWriter};
use talos_store::session::{SessionArtifactStore, SessionStore};
use talos_tools::ToolRegistry;
use talos_types::ModelSelection;

use crate::{agent::AgentHarnessParams, AgentHarnessImpl, SkillRootsResolver};

/// Supported Agent runtime families.
///
/// Adding a new kind requires a matching typed runtime_config variant and an
/// AgentRuntime implementation. Do not route unsupported kinds to Kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Kernel,
    Codex,
    Claude,
}

impl From<ConfigAgentKind> for AgentKind {
    fn from(kind: ConfigAgentKind) -> Self {
        match kind {
            ConfigAgentKind::Kernel => Self::Kernel,
            ConfigAgentKind::Codex => Self::Codex,
            ConfigAgentKind::Claude => Self::Claude,
        }
    }
}

/// Capability flags used by Coordinator and Harness validation.
///
/// These flags are not presentation-only metadata. Unsupported capabilities
/// must fail fast before a request reaches a concrete runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCapabilities {
    /// The Agent can stream standardized AgentEvent values.
    pub streaming: bool,
    /// The Agent can accept interrupt requests for active runs.
    pub interrupt: bool,
    /// The Agent can accept structured continuation input.
    pub dynamic_input: bool,
}

impl AgentCapabilities {
    /// Conservative default for Harness implementations that do not advertise capabilities.
    pub fn none() -> Self {
        Self {
            streaming: false,
            interrupt: false,
            dynamic_input: false,
        }
    }

    /// Capabilities exposed by the built-in Kernel-backed Agent runtime.
    pub fn kernel() -> Self {
        Self {
            streaming: true,
            interrupt: true,
            dynamic_input: true,
        }
    }
}

/// Lifecycle status of an Agent definition in the Harness framework.
///
/// This is not product session state and must not mirror Service active-turn
/// persistence. Runtime health can be added later as a separate concept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    Enabled,
    Disabled,
}

/// Resolved Agent definition used to build an AgentHarness.
///
/// This is the Harness domain object produced from configuration. It must not
/// carry store records, Gateway DTOs, or concrete runtime clients.
#[derive(Debug, Clone)]
pub struct AgentDefinition {
    /// Stable Agent identity used by Coordinator routing.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Runtime family selected for this Agent.
    pub kind: AgentKind,
    /// Whether this definition should be instantiated.
    pub enabled: bool,
    /// Human-readable description for management surfaces.
    pub description: String,
    /// Default execution workspace key, not a filesystem path and not an Agent selector.
    pub default_workspace_id: Option<String>,
    /// Concrete model and reasoning policy used to initialize this Agent runtime.
    pub model_selection: ModelSelection,
    /// Typed runtime configuration matched with kind.
    pub runtime_config: AgentRuntimeConfig,
}

/// Management descriptor exposed by AgentHarness and AgentRegistry.
///
/// Descriptors support routing, capability validation, and UI/SDK management
/// surfaces. They must not expose runtime-private handles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDescriptor {
    /// Stable Agent identity used by Coordinator routing.
    pub agent_id: String,
    /// Human-readable display name.
    pub name: String,
    /// Runtime family for the Agent.
    pub kind: AgentKind,
    /// Human-readable description.
    pub description: String,
    /// Capabilities available through the standard Harness API.
    pub capabilities: AgentCapabilities,
    /// Definition lifecycle status.
    pub status: AgentStatus,
}

/// Loads AgentDefinition values from product configuration.
///
/// The loader owns config-to-domain validation only. It does not instantiate
/// runtimes, register Agents, or read stores.
pub struct AgentDefinitionLoader;

impl AgentDefinitionLoader {
    /// Load explicit definitions or synthesize the default Kernel Agent.
    pub fn load(
        config: &AgentConfig,
        default_model_selection: &ModelSelection,
    ) -> TalosResult<Vec<AgentDefinition>> {
        if config.agents.is_empty() {
            return Ok(vec![synthesized_default_definition(
                config,
                default_model_selection,
            )]);
        }

        let mut seen_ids = HashSet::new();
        config
            .agents
            .iter()
            .map(|definition| {
                let definition =
                    definition_from_config(config, default_model_selection, definition)?;
                if !seen_ids.insert(definition.id.clone()) {
                    return Err(TalosError::public(
                        talos_error::ErrorCode::InvalidRequest,
                        format!("duplicate agent definition id: {}", definition.id),
                    ));
                }
                Ok(definition)
            })
            .collect()
    }
}

/// Builds AgentHarness instances from resolved AgentDefinition values.
///
/// The factory belongs to the Harness layer. It assembles shared infrastructure
/// and selects the concrete runtime implementation without leaking runtime
/// internals to Coordinator.
pub struct AgentHarnessFactory {
    llm_provider_router: Arc<dyn LlmProviderRouter>,
    tool_registry: Arc<ToolRegistry>,
    skill_roots_resolver: Arc<dyn SkillRootsResolver>,
    session_artifact_store: Arc<dyn SessionArtifactStore>,
    session_store: Arc<dyn SessionStore>,
    memory_writer: Arc<dyn MemoryWriter>,
    user_workspace_memory_reader: Arc<dyn UserWorkspaceMemoryReader>,
    user_workspace_memory_writer: Arc<dyn UserWorkspaceMemoryWriter>,
    attachment_reader: Arc<dyn AttachmentReader>,
    inner_model: Arc<crate::InnerModelClient>,
}

/// Explicit dependencies shared by every Harness built by one factory.
pub struct AgentHarnessFactoryParams {
    pub llm_provider_router: Arc<dyn LlmProviderRouter>,
    pub tool_registry: Arc<ToolRegistry>,
    pub skill_roots_resolver: Arc<dyn SkillRootsResolver>,
    pub session_artifact_store: Arc<dyn SessionArtifactStore>,
    pub session_store: Arc<dyn SessionStore>,
    pub memory_writer: Arc<dyn MemoryWriter>,
    pub user_workspace_memory_reader: Arc<dyn UserWorkspaceMemoryReader>,
    pub user_workspace_memory_writer: Arc<dyn UserWorkspaceMemoryWriter>,
    pub attachment_reader: Arc<dyn AttachmentReader>,
    pub inner_model: Arc<crate::InnerModelClient>,
}

impl AgentHarnessFactory {
    /// Create a factory with shared Harness infrastructure.
    pub fn new(params: AgentHarnessFactoryParams) -> Self {
        let AgentHarnessFactoryParams {
            llm_provider_router,
            tool_registry,
            skill_roots_resolver,
            session_artifact_store,
            session_store,
            memory_writer,
            user_workspace_memory_reader,
            user_workspace_memory_writer,
            attachment_reader,
            inner_model,
        } = params;
        Self {
            llm_provider_router,
            tool_registry,
            skill_roots_resolver,
            session_artifact_store,
            session_store,
            memory_writer,
            user_workspace_memory_reader,
            user_workspace_memory_writer,
            attachment_reader,
            inner_model,
        }
    }

    /// Build a runnable Harness for an enabled Agent definition.
    ///
    /// Unsupported runtime kinds must fail explicitly instead of falling back to
    /// Kernel, otherwise routing and capability validation become misleading.
    pub fn build(&self, definition: &AgentDefinition) -> TalosResult<AgentHarnessImpl> {
        if !definition.enabled {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("agent `{}` is disabled", definition.id),
            ));
        }
        match (&definition.kind, &definition.runtime_config) {
            (AgentKind::Kernel, AgentRuntimeConfig::Kernel { kernel }) => {
                AgentHarnessImpl::new(AgentHarnessParams {
                    id: definition.id.clone(),
                    name: definition.name.clone(),
                    description: definition.description.clone(),
                    llm_provider_router: self.llm_provider_router.clone(),
                    model_selection: definition.model_selection.clone(),
                    inner_model: self.inner_model.clone(),
                    tool_registry: self.tool_registry.clone(),
                    kernel_config: kernel.clone(),
                    skill_roots_resolver: self.skill_roots_resolver.clone(),
                    session_artifact_store: self.session_artifact_store.clone(),
                    session_store: self.session_store.clone(),
                    memory_writer: self.memory_writer.clone(),
                    user_workspace_memory_reader: self.user_workspace_memory_reader.clone(),
                    user_workspace_memory_writer: self.user_workspace_memory_writer.clone(),
                    attachment_reader: self.attachment_reader.clone(),
                })
            }
            (kind, _) => Err(TalosError::public(
                talos_error::ErrorCode::ProviderUnavailable,
                format!("agent runtime kind is not implemented: {:?}", kind),
            )),
        }
    }
}

impl AgentDefinition {
    /// Convert the definition into a routing and management descriptor.
    pub fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            agent_id: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind,
            description: self.description.clone(),
            capabilities: match self.kind {
                AgentKind::Kernel => AgentCapabilities::kernel(),
                AgentKind::Codex | AgentKind::Claude => AgentCapabilities {
                    streaming: false,
                    interrupt: false,
                    dynamic_input: false,
                },
            },
            status: if self.enabled {
                AgentStatus::Enabled
            } else {
                AgentStatus::Disabled
            },
        }
    }
}

fn synthesized_default_definition(
    config: &AgentConfig,
    default_model_selection: &ModelSelection,
) -> AgentDefinition {
    AgentDefinition {
        id: "default".to_string(),
        name: config.name.clone(),
        kind: AgentKind::Kernel,
        enabled: true,
        description: "Default AI assistant".to_string(),
        default_workspace_id: Some(config.kernel.default_workspace_id.clone()),
        model_selection: default_model_selection.clone(),
        runtime_config: AgentRuntimeConfig::Kernel {
            kernel: config.kernel.clone(),
        },
    }
}

fn definition_from_config(
    config: &AgentConfig,
    default_model_selection: &ModelSelection,
    definition: &AgentDefinitionConfig,
) -> TalosResult<AgentDefinition> {
    let id = definition.id.trim();
    if id.is_empty() {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent definition id cannot be empty".to_string(),
        ));
    }
    let kind = AgentKind::from(definition.kind);
    let runtime_config =
        definition
            .runtime_config
            .clone()
            .unwrap_or_else(|| AgentRuntimeConfig::Kernel {
                kernel: config.kernel.clone(),
            });
    let runtime_kind = kind_for_runtime_config(&runtime_config);
    if kind != runtime_kind {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            format!(
                "agent `{id}` kind {:?} does not match runtime_config {:?}",
                kind, runtime_kind
            ),
        ));
    }

    Ok(AgentDefinition {
        id: id.to_string(),
        name: non_empty_or(&definition.name, id),
        kind,
        enabled: definition.enabled,
        description: definition
            .description
            .clone()
            .unwrap_or_else(|| format!("{} agent", non_empty_or(&definition.name, id))),
        default_workspace_id: definition
            .default_workspace_id
            .clone()
            .or_else(|| Some(config.kernel.default_workspace_id.clone())),
        model_selection: default_model_selection.clone(),
        runtime_config,
    })
}

fn non_empty_or(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn kind_for_runtime_config(runtime_config: &AgentRuntimeConfig) -> AgentKind {
    match runtime_config {
        AgentRuntimeConfig::Kernel { .. } => AgentKind::Kernel,
    }
}

#[cfg(test)]
#[path = "definition_tests.rs"]
mod tests;
