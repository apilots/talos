use std::sync::Arc;

use talos_config::{AgentKernelConfig, ModelInputKind, ProviderModelConfig};
use talos_llm_provider::LlmProviderRouter;
use talos_memory::{
    DreamMemoryFormationStore, MemoryAgentId, MemoryScope, UserWorkspaceMemoryDocument,
    UserWorkspaceMemoryEntry, UserWorkspaceMemoryEntryQuery, UserWorkspaceMemoryEntryUpdate,
    UserWorkspaceMemoryProducer, UserWorkspaceMemoryReader, UserWorkspaceMemorySnapshot,
    UserWorkspaceMemoryWriter,
};
use talos_store::session::FileSessionArtifactStore;
use talos_tools::ToolRegistry;
use talos_types::ExpectedFileVersion;
use talos_types::{ModelInfo, ModelSelection};

use super::{AgentHarnessImpl, AgentHarnessParams};

/// Memory writer used by Agent Harness tests that do not inspect persisted memory.
pub(super) struct TestMemoryWriter;

pub(super) struct TestUserWorkspaceMemory;

fn test_user_workspace_memory_snapshot() -> UserWorkspaceMemorySnapshot {
    UserWorkspaceMemorySnapshot::Missing {
        template: UserWorkspaceMemoryDocument::template(UserWorkspaceMemoryProducer::new(
            MemoryAgentId::parse("agent-1").expect("test Agent id should be valid"),
            1_784_251_800,
        ))
        .expect("test User Workspace Memory template should build"),
    }
}

impl UserWorkspaceMemoryReader for TestUserWorkspaceMemory {
    fn read<'a>(
        &'a self,
        _scope: &'a MemoryScope,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = talos_error::TalosResult<UserWorkspaceMemorySnapshot>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { Ok(test_user_workspace_memory_snapshot()) })
    }

    fn get_entry<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        _key: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = talos_error::TalosResult<Option<UserWorkspaceMemoryEntry>>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async { Ok(None) })
    }

    fn list_entries<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        _query: UserWorkspaceMemoryEntryQuery,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = talos_error::TalosResult<Vec<UserWorkspaceMemoryEntry>>,
                > + Send
                + 'a,
        >,
    > {
        Box::pin(async { Ok(Vec::new()) })
    }
}

macro_rules! test_user_workspace_memory_write {
    ($name:ident($($arg:ident: $ty:ty),*)) => {
        fn $name<'a>(
            &'a self,
            _scope: &'a MemoryScope,
            $($arg: $ty),*
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = talos_error::TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>> {
            Box::pin(async { Ok(test_user_workspace_memory_snapshot()) })
        }
    };
}

impl UserWorkspaceMemoryWriter for TestUserWorkspaceMemory {
    test_user_workspace_memory_write!(create_entry(_entry: UserWorkspaceMemoryEntry));
    test_user_workspace_memory_write!(update_entry(_key: &'a str, _update: UserWorkspaceMemoryEntryUpdate));
    test_user_workspace_memory_write!(upsert_entry(_entry: UserWorkspaceMemoryEntry));
    test_user_workspace_memory_write!(replace_entry(_entry: UserWorkspaceMemoryEntry));
    test_user_workspace_memory_write!(remove_entry(_key: &'a str));
    test_user_workspace_memory_write!(replace_document(_content: String, _base_version: ExpectedFileVersion));
}

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
        dream_memory_store: Arc::new(TestMemoryWriter),
        user_workspace_memory_reader: Arc::new(TestUserWorkspaceMemory),
        user_workspace_memory_writer: Arc::new(TestUserWorkspaceMemory),
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
