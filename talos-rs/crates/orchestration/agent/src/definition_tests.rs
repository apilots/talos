use std::sync::Arc;

use talos_config::{
    AgentConfig, AgentDefinitionConfig, AgentKind as ConfigAgentKind, AgentRuntimeConfig,
};
use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Usage};
use talos_tools::ToolRegistry;

use super::*;
use crate::{empty_skill_roots_resolver, AgentHarness};

struct TestLlmProvider;

#[async_trait::async_trait]
impl LlmProvider for TestLlmProvider {
    fn name(&self) -> &str {
        "test"
    }

    async fn chat(&self, _request: ChatRequest) -> talos_error::TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: String::new(),
            reasoning: Vec::new(),
            model: "test-model".to_string(),
            usage: Usage::default(),
            finish_reason: None,
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> talos_error::TalosResult<ChatStream> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(ChatStream { rx })
    }

    fn default_model(&self) -> String {
        "test-model".to_string()
    }
}

#[test]
fn loader_builds_synthesized_default_definition() {
    let mut config = AgentConfig::default();
    config.name = "Talos".to_string();
    config.model = "agent-model".to_string();
    config.kernel.default_workspace_id = "workspace-default".to_string();

    let definitions =
        AgentDefinitionLoader::load(&config, "global-model").expect("definitions should load");

    assert_eq!(definitions.len(), 1);
    let definition = &definitions[0];
    assert_eq!(definition.id, "default");
    assert_eq!(definition.name, "Talos");
    assert_eq!(definition.kind, AgentKind::Kernel);
    assert_eq!(
        definition.default_workspace_id.as_deref(),
        Some("workspace-default")
    );
    assert_eq!(definition.model.as_deref(), Some("agent-model"));
}

#[test]
fn loader_resolves_explicit_agent_definition() {
    let mut config = AgentConfig::default();
    config.kernel.default_workspace_id = "base-workspace".to_string();
    config.agents = vec![AgentDefinitionConfig {
        id: "reviewer".to_string(),
        name: "Reviewer".to_string(),
        kind: ConfigAgentKind::Kernel,
        enabled: true,
        description: None,
        default_workspace_id: None,
        model: None,
        runtime_config: Some(AgentRuntimeConfig::Kernel {
            kernel: config.kernel.clone(),
        }),
    }];

    let definitions =
        AgentDefinitionLoader::load(&config, "global-model").expect("definitions should load");

    assert_eq!(definitions.len(), 1);
    let definition = &definitions[0];
    assert_eq!(definition.id, "reviewer");
    assert_eq!(definition.name, "Reviewer");
    assert_eq!(definition.description, "Reviewer agent");
    assert_eq!(
        definition.default_workspace_id.as_deref(),
        Some("base-workspace")
    );
    assert_eq!(definition.model.as_deref(), Some("global-model"));
}

#[test]
fn loader_rejects_duplicate_agent_ids() {
    let mut config = AgentConfig::default();
    config.agents = vec![
        AgentDefinitionConfig {
            id: "duplicate".to_string(),
            name: "First".to_string(),
            kind: ConfigAgentKind::Kernel,
            enabled: true,
            description: None,
            default_workspace_id: None,
            model: None,
            runtime_config: None,
        },
        AgentDefinitionConfig {
            id: "duplicate".to_string(),
            name: "Second".to_string(),
            kind: ConfigAgentKind::Kernel,
            enabled: true,
            description: None,
            default_workspace_id: None,
            model: None,
            runtime_config: None,
        },
    ];

    let error =
        AgentDefinitionLoader::load(&config, "global-model").expect_err("duplicate id should fail");

    assert!(error.to_string().contains("duplicate"));
}

#[test]
fn loader_rejects_kind_runtime_mismatch() {
    let mut config = AgentConfig::default();
    config.agents = vec![AgentDefinitionConfig {
        id: "codex".to_string(),
        name: "Codex".to_string(),
        kind: ConfigAgentKind::Codex,
        enabled: true,
        description: None,
        default_workspace_id: None,
        model: None,
        runtime_config: None,
    }];

    let error = AgentDefinitionLoader::load(&config, "global-model")
        .expect_err("kind/runtime mismatch should fail");

    assert!(error.to_string().contains("does not match"));
}

#[test]
fn factory_rejects_disabled_definition() {
    let mut config = AgentConfig::default();
    config.agents = vec![AgentDefinitionConfig {
        id: "disabled".to_string(),
        name: "Disabled".to_string(),
        kind: ConfigAgentKind::Kernel,
        enabled: false,
        description: None,
        default_workspace_id: None,
        model: None,
        runtime_config: None,
    }];
    let definition = AgentDefinitionLoader::load(&config, "global-model")
        .expect("definitions should load")
        .remove(0);
    let factory = AgentHarnessFactory::new(
        Arc::new(TestLlmProvider),
        Arc::new(ToolRegistry::new()),
        empty_skill_roots_resolver(),
    );

    let error = match factory.build(&definition) {
        Ok(_) => panic!("disabled definition should be rejected"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("disabled"));
}

#[test]
fn factory_builds_kernel_agent_with_descriptor() {
    let config = AgentConfig::default();
    let definition = AgentDefinitionLoader::load(&config, "global-model")
        .expect("definitions should load")
        .remove(0);
    let factory = AgentHarnessFactory::new(
        Arc::new(TestLlmProvider),
        Arc::new(ToolRegistry::new()),
        empty_skill_roots_resolver(),
    );

    let agent = factory.build(&definition).expect("agent should build");
    let descriptor = agent.descriptor();

    assert_eq!(descriptor.agent_id, "default");
    assert_eq!(descriptor.kind, AgentKind::Kernel);
    assert!(descriptor.capabilities.streaming);
}
