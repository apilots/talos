use std::collections::HashSet;

use talos_config::{AgentConfig, AgentDefinitionConfig, AgentProviderConfig};
use talos_error::{TalosError, TalosResult};

/// Resolved Agent definition used to build an AgentHarness.
///
/// This is the Harness domain object produced from configuration. It must not
/// carry store records, Gateway DTOs, or concrete Provider clients.
#[derive(Debug, Clone)]
pub struct AgentDefinition {
    /// Stable Agent identity used by Coordinator routing.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Whether this definition should be instantiated.
    pub enabled: bool,
    /// Human-readable description for management surfaces.
    pub description: String,
    /// Typed Provider configuration selected for this Agent.
    pub provider: AgentProviderConfig,
}

/// Loads AgentDefinition values from product configuration.
///
/// The loader owns config-to-domain validation only. It does not instantiate
/// Provider implementations, register Agents, or read stores.
pub struct AgentDefinitionLoader;

impl AgentDefinitionLoader {
    /// Load explicit definitions or synthesize the default Kernel Agent.
    pub fn load(config: &AgentConfig) -> TalosResult<Vec<AgentDefinition>> {
        if config.agents.is_empty() {
            return Ok(vec![synthesized_default_definition(config)]);
        }

        let mut seen_ids = HashSet::new();
        config
            .agents
            .iter()
            .map(|definition| {
                let definition = definition_from_config(config, definition)?;
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

fn synthesized_default_definition(config: &AgentConfig) -> AgentDefinition {
    AgentDefinition {
        id: "default".to_string(),
        name: config.name.clone(),
        enabled: true,
        description: "Default AI assistant".to_string(),
        provider: AgentProviderConfig::Kernel {
            kernel: config.kernel.clone(),
        },
    }
}

fn definition_from_config(
    config: &AgentConfig,
    definition: &AgentDefinitionConfig,
) -> TalosResult<AgentDefinition> {
    let id = definition.id.trim();
    if id.is_empty() {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "agent definition id cannot be empty".to_string(),
        ));
    }
    let provider = definition
        .provider
        .clone()
        .unwrap_or_else(|| AgentProviderConfig::Kernel {
            kernel: config.kernel.clone(),
        });

    Ok(AgentDefinition {
        id: id.to_string(),
        name: non_empty_or(&definition.name, id),
        enabled: definition.enabled,
        description: definition
            .description
            .clone()
            .unwrap_or_else(|| format!("{} agent", non_empty_or(&definition.name, id))),
        provider,
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

#[cfg(test)]
#[path = "definition_tests.rs"]
mod tests;
