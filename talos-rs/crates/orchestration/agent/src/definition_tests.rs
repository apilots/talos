use std::sync::Arc;

use talos_config::{
    AgentConfig, AgentDefinitionConfig, AgentKernelConfig, AgentKind as ConfigAgentKind,
    AgentRuntimeConfig,
};
use talos_llm_provider::{
    ChatRequest, ChatResponse, ChatStream, LlmProvider, LlmProviderRouter, ReasoningOutput, Usage,
};
use talos_memory::{
    DreamMemoryFormationStore, MemoryScope, UserWorkspaceMemoryEntry,
    UserWorkspaceMemoryEntryQuery, UserWorkspaceMemoryEntryUpdate, UserWorkspaceMemoryReader,
    UserWorkspaceMemorySnapshot, UserWorkspaceMemoryWriter,
};
use talos_store::session::FileSessionArtifactStore;
use talos_tools::ToolRegistry;
use talos_types::{ExpectedFileVersion, ModelRef, ModelSelection};

use super::*;
use crate::{empty_skill_roots_resolver, AgentHarness};

struct TestLlmProvider;

struct TestMemoryWriter;
struct TestUserWorkspaceMemory;

impl DreamMemoryFormationStore for TestMemoryWriter {
    fn read_day<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        date: talos_memory::DreamMemoryDate,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = talos_error::TalosResult<talos_memory::DreamMemoryDaySnapshot>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            Ok(talos_memory::DreamMemoryDaySnapshot::new(
                date,
                talos_memory::DreamMemoryDayRevision::Missing,
                Vec::new(),
            ))
        })
    }

    fn write<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        _request: talos_memory::DreamMemoryWriteRequest,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = talos_error::TalosResult<()>> + Send + 'a>,
    > {
        Box::pin(async { Ok(()) })
    }
}

macro_rules! unused_user_workspace_memory_method {
    ($name:ident($($arg:ident: $ty:ty),*) -> $output:ty) => {
        fn $name<'a>(
            &'a self,
            _scope: &'a MemoryScope,
            $($arg: $ty),*
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = talos_error::TalosResult<$output>> + Send + 'a>> {
            Box::pin(async { unreachable!("factory test must not access User Workspace Memory") })
        }
    };
}

impl UserWorkspaceMemoryReader for TestUserWorkspaceMemory {
    unused_user_workspace_memory_method!(read() -> UserWorkspaceMemorySnapshot);
    unused_user_workspace_memory_method!(get_entry(_key: &'a str) -> Option<UserWorkspaceMemoryEntry>);
    unused_user_workspace_memory_method!(list_entries(_query: UserWorkspaceMemoryEntryQuery) -> Vec<UserWorkspaceMemoryEntry>);
}

impl UserWorkspaceMemoryWriter for TestUserWorkspaceMemory {
    unused_user_workspace_memory_method!(create_entry(_entry: UserWorkspaceMemoryEntry) -> UserWorkspaceMemorySnapshot);
    unused_user_workspace_memory_method!(update_entry(_key: &'a str, _update: UserWorkspaceMemoryEntryUpdate) -> UserWorkspaceMemorySnapshot);
    unused_user_workspace_memory_method!(upsert_entry(_entry: UserWorkspaceMemoryEntry) -> UserWorkspaceMemorySnapshot);
    unused_user_workspace_memory_method!(replace_entry(_entry: UserWorkspaceMemoryEntry) -> UserWorkspaceMemorySnapshot);
    unused_user_workspace_memory_method!(remove_entry(_key: &'a str) -> UserWorkspaceMemorySnapshot);
    unused_user_workspace_memory_method!(replace_document(_content: String, _base_version: ExpectedFileVersion) -> UserWorkspaceMemorySnapshot);
}

#[async_trait::async_trait]
impl LlmProvider for TestLlmProvider {
    fn name(&self) -> &str {
        "test"
    }

    async fn chat(&self, _request: ChatRequest) -> talos_error::TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: String::new(),
            reasoning: ReasoningOutput::default(),
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

struct TestLlmProviderRouter;

impl LlmProviderRouter for TestLlmProviderRouter {
    fn provider_for(
        &self,
        _model_ref: &ModelRef,
    ) -> talos_error::TalosResult<Arc<dyn LlmProvider>> {
        Ok(Arc::new(TestLlmProvider))
    }
}

fn default_model_selection() -> ModelSelection {
    ModelSelection::without_reasoning(ModelRef {
        provider: "test".to_string(),
        id: "global-model".to_string(),
    })
}

fn inner_model() -> Arc<crate::InnerModelClient> {
    let selection = default_model_selection();
    Arc::new(
        crate::InnerModelClient::builder()
            .configured_model(None)
            .agent_default_model(selection.model_ref.clone())
            .model_catalog(vec![talos_types::ModelInfo::from_provider_model(
                selection.model_ref.provider.clone(),
                serde_json::from_value(serde_json::json!({"id": selection.model_ref.id})).unwrap(),
            )])
            .provider_router(Arc::new(TestLlmProviderRouter))
            .profile_store(Arc::new(talos_store::profile::MockProfileStore::default()))
            .profile_timezone("UTC".to_string())
            .build()
            .unwrap(),
    )
}

#[test]
fn loader_builds_synthesized_default_definition() {
    let config = AgentConfig {
        name: "Talos".to_string(),
        kernel: AgentKernelConfig {
            default_workspace_id: "workspace-default".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };

    let definitions = AgentDefinitionLoader::load(&config, &default_model_selection())
        .expect("definitions should load");

    assert_eq!(definitions.len(), 1);
    let definition = &definitions[0];
    assert_eq!(definition.id, "default");
    assert_eq!(definition.name, "Talos");
    assert_eq!(definition.kind, AgentKind::Kernel);
    assert_eq!(
        definition.default_workspace_id.as_deref(),
        Some("workspace-default")
    );
    assert_eq!(definition.model_selection, default_model_selection());
}

#[test]
fn loader_resolves_explicit_agent_definition() {
    let kernel = AgentKernelConfig {
        default_workspace_id: "base-workspace".to_string(),
        ..Default::default()
    };
    let config = AgentConfig {
        kernel: kernel.clone(),
        agents: vec![AgentDefinitionConfig {
            id: "reviewer".to_string(),
            name: "Reviewer".to_string(),
            kind: ConfigAgentKind::Kernel,
            enabled: true,
            description: None,
            default_workspace_id: None,
            runtime_config: Some(AgentRuntimeConfig::Kernel { kernel }),
        }],
        ..Default::default()
    };

    let definitions = AgentDefinitionLoader::load(&config, &default_model_selection())
        .expect("definitions should load");

    assert_eq!(definitions.len(), 1);
    let definition = &definitions[0];
    assert_eq!(definition.id, "reviewer");
    assert_eq!(definition.name, "Reviewer");
    assert_eq!(definition.description, "Reviewer agent");
    assert_eq!(
        definition.default_workspace_id.as_deref(),
        Some("base-workspace")
    );
    assert_eq!(definition.model_selection, default_model_selection());
}

#[test]
fn loader_rejects_duplicate_agent_ids() {
    let config = AgentConfig {
        agents: vec![
            AgentDefinitionConfig {
                id: "duplicate".to_string(),
                name: "First".to_string(),
                kind: ConfigAgentKind::Kernel,
                enabled: true,
                description: None,
                default_workspace_id: None,
                runtime_config: None,
            },
            AgentDefinitionConfig {
                id: "duplicate".to_string(),
                name: "Second".to_string(),
                kind: ConfigAgentKind::Kernel,
                enabled: true,
                description: None,
                default_workspace_id: None,
                runtime_config: None,
            },
        ],
        ..Default::default()
    };

    let error = AgentDefinitionLoader::load(&config, &default_model_selection())
        .expect_err("duplicate id should fail");

    assert!(error.to_string().contains("duplicate"));
}

#[test]
fn loader_rejects_kind_runtime_mismatch() {
    let config = AgentConfig {
        agents: vec![AgentDefinitionConfig {
            id: "codex".to_string(),
            name: "Codex".to_string(),
            kind: ConfigAgentKind::Codex,
            enabled: true,
            description: None,
            default_workspace_id: None,
            runtime_config: None,
        }],
        ..Default::default()
    };

    let error = AgentDefinitionLoader::load(&config, &default_model_selection())
        .expect_err("kind/runtime mismatch should fail");

    assert!(error.to_string().contains("does not match"));
}

#[test]
fn factory_rejects_disabled_definition() {
    let config = AgentConfig {
        agents: vec![AgentDefinitionConfig {
            id: "disabled".to_string(),
            name: "Disabled".to_string(),
            kind: ConfigAgentKind::Kernel,
            enabled: false,
            description: None,
            default_workspace_id: None,
            runtime_config: None,
        }],
        ..Default::default()
    };
    let definition = AgentDefinitionLoader::load(&config, &default_model_selection())
        .expect("definitions should load")
        .remove(0);
    let factory = AgentHarnessFactory::new(AgentHarnessFactoryParams {
        llm_provider_router: Arc::new(TestLlmProviderRouter),
        tool_registry: Arc::new(ToolRegistry::new()),
        skill_roots_resolver: empty_skill_roots_resolver(),
        session_artifact_store: Arc::new(FileSessionArtifactStore::default()),
        session_store: Arc::new(talos_store::session::MockSessionStore::new()),
        dream_memory_store: Arc::new(TestMemoryWriter),
        user_workspace_memory_reader: Arc::new(TestUserWorkspaceMemory),
        user_workspace_memory_writer: Arc::new(TestUserWorkspaceMemory),
        attachment_reader: Arc::new(talos_agent_kernel::RejectingAttachmentReader),
        inner_model: inner_model(),
    });

    let error = match factory.build(&definition) {
        Ok(_) => panic!("disabled definition should be rejected"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("disabled"));
}

#[test]
fn factory_builds_kernel_agent_with_descriptor() {
    let config = AgentConfig::default();
    let definition = AgentDefinitionLoader::load(&config, &default_model_selection())
        .expect("definitions should load")
        .remove(0);
    let factory = AgentHarnessFactory::new(AgentHarnessFactoryParams {
        llm_provider_router: Arc::new(TestLlmProviderRouter),
        tool_registry: Arc::new(ToolRegistry::new()),
        skill_roots_resolver: empty_skill_roots_resolver(),
        session_artifact_store: Arc::new(FileSessionArtifactStore::default()),
        session_store: Arc::new(talos_store::session::MockSessionStore::new()),
        dream_memory_store: Arc::new(TestMemoryWriter),
        user_workspace_memory_reader: Arc::new(TestUserWorkspaceMemory),
        user_workspace_memory_writer: Arc::new(TestUserWorkspaceMemory),
        attachment_reader: Arc::new(talos_agent_kernel::RejectingAttachmentReader),
        inner_model: inner_model(),
    });

    let agent = factory.build(&definition).expect("agent should build");
    let descriptor = agent.descriptor();

    assert_eq!(descriptor.agent_id, "default");
    assert_eq!(descriptor.kind, AgentKind::Kernel);
    assert!(descriptor.capabilities.streaming);
}
