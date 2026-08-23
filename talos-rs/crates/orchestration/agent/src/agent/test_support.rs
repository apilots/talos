use std::sync::Arc;

use talos_config::{AgentKernelConfig, ModelInputKind, ProviderModelConfig};
use talos_llm_provider::LlmProviderRouter;
use talos_memory::{MemoryScope, MemoryWriteBatch, MemoryWriter};
use talos_store::session::FileSessionArtifactStore;
use talos_tools::ToolRegistry;
use talos_types::{ModelInfo, ModelSelection};

use super::{AgentHarnessImpl, AgentHarnessParams};

/// Memory writer used by Agent Harness tests that do not inspect persisted memory.
pub(super) struct TestMemoryWriter;

impl MemoryWriter for TestMemoryWriter {
    fn write<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        _batch: MemoryWriteBatch,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = talos_error::TalosResult<()>> + Send + 'a>,
    > {
        Box::pin(async { Ok(()) })
    }
}

/// Builds the standard Agent Harness fixture shared by Agent behavior tests.
pub(super) fn test_agent_harness(
    id: String,
    name: String,
    description: String,
    llm_provider_router: Arc<dyn LlmProviderRouter>,
    model_selection: ModelSelection,
    tool_registry: Arc<ToolRegistry>,
    kernel_config: AgentKernelConfig,
) -> talos_error::TalosResult<AgentHarnessImpl> {
    let inner_model = test_inner_model(llm_provider_router.clone(), &model_selection)?;
    AgentHarnessImpl::new(AgentHarnessParams {
        id,
        name,
        description,
        llm_provider_router,
        model_selection,
        inner_model,
        tool_registry,
        kernel_config,
        skill_roots_resolver: crate::empty_skill_roots_resolver(),
        session_artifact_store: Arc::new(FileSessionArtifactStore::default()),
        session_store: Arc::new(talos_store::session::MockSessionStore::new()),
        memory_writer: Arc::new(TestMemoryWriter),
        attachment_reader: Arc::new(talos_agent_kernel::RejectingAttachmentReader),
    })
}

/// Builds an Inner Model client on the same Provider Router used by the Agent test.
pub(super) fn test_inner_model(
    router: Arc<dyn LlmProviderRouter>,
    selection: &ModelSelection,
) -> talos_error::TalosResult<Arc<crate::InnerModelClient>> {
    Ok(Arc::new(
        crate::InnerModelClient::builder()
            .configured_model(None)
            .agent_default_model(selection.model_ref.clone())
            .model_catalog(vec![ModelInfo::from_provider_model(
                selection.model_ref.provider.clone(),
                ProviderModelConfig {
                    id: selection.model_ref.id.clone(),
                    name: selection.model_ref.id.clone(),
                    context_window: 128_000,
                    reasoning: false,
                    reasoning_effort: Default::default(),
                    speed: Default::default(),
                    input: vec![ModelInputKind::Text],
                    cost: Default::default(),
                },
            )])
            .provider_router(router)
            .profile_store(Arc::new(talos_store::profile::MockProfileStore::default()))
            .profile_timezone("UTC".to_string())
            .build()?,
    ))
}
