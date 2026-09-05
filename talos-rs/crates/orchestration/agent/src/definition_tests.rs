use talos_config::{AgentConfig, AgentDefinitionConfig, AgentKernelConfig, AgentProviderConfig};

use super::*;

#[test]
fn loader_builds_synthesized_default_definition() {
    let config = AgentConfig {
        name: "Talos".to_string(),
        ..Default::default()
    };

    let definitions = AgentDefinitionLoader::load(&config).expect("definitions should load");

    assert_eq!(definitions.len(), 1);
    let definition = &definitions[0];
    assert_eq!(definition.id, "default");
    assert_eq!(definition.name, "Talos");
}

#[test]
fn loader_resolves_explicit_agent_definition() {
    let kernel = AgentKernelConfig::default();
    let config = AgentConfig {
        kernel: kernel.clone(),
        agents: vec![AgentDefinitionConfig {
            id: "reviewer".to_string(),
            name: "Reviewer".to_string(),
            enabled: true,
            description: None,
            provider: Some(AgentProviderConfig::Kernel { kernel }),
        }],
        ..Default::default()
    };

    let definitions = AgentDefinitionLoader::load(&config).expect("definitions should load");

    assert_eq!(definitions.len(), 1);
    let definition = &definitions[0];
    assert_eq!(definition.id, "reviewer");
    assert_eq!(definition.name, "Reviewer");
    assert_eq!(definition.description, "Reviewer agent");
}

#[test]
fn loader_rejects_duplicate_agent_ids() {
    let config = AgentConfig {
        agents: vec![
            AgentDefinitionConfig {
                id: "duplicate".to_string(),
                name: "First".to_string(),
                enabled: true,
                description: None,
                provider: None,
            },
            AgentDefinitionConfig {
                id: "duplicate".to_string(),
                name: "Second".to_string(),
                enabled: true,
                description: None,
                provider: None,
            },
        ],
        ..Default::default()
    };

    let error = AgentDefinitionLoader::load(&config).expect_err("duplicate id should fail");

    assert!(error.to_string().contains("duplicate"));
}
