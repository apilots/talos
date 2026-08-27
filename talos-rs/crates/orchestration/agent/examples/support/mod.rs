use std::sync::Arc;

use talos_agent::InnerModelClient;
use talos_llm_provider::LlmProviderRouter;
use talos_memory::{
    MemoryScope, MemoryWriteBatch, MemoryWriter, UserWorkspaceMemoryDocument,
    UserWorkspaceMemoryEntry, UserWorkspaceMemoryEntryQuery, UserWorkspaceMemoryEntryUpdate,
    UserWorkspaceMemoryReader, UserWorkspaceMemorySnapshot, UserWorkspaceMemoryWriter,
};
use talos_types::{ExpectedFileVersion, ModelInfo, ModelSelection};

pub struct NoopMemoryWriter;
pub struct NoopUserWorkspaceMemory;

fn missing_snapshot() -> UserWorkspaceMemorySnapshot {
    UserWorkspaceMemorySnapshot::Missing {
        template: UserWorkspaceMemoryDocument::template()
            .expect("example User Workspace Memory template should build"),
    }
}

impl UserWorkspaceMemoryReader for NoopUserWorkspaceMemory {
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
        Box::pin(async { Ok(missing_snapshot()) })
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

macro_rules! noop_user_workspace_memory_write {
    ($name:ident($($arg:ident: $ty:ty),*)) => {
        fn $name<'a>(
            &'a self,
            _scope: &'a MemoryScope,
            $($arg: $ty),*
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = talos_error::TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>> {
            Box::pin(async { Ok(missing_snapshot()) })
        }
    };
}

impl UserWorkspaceMemoryWriter for NoopUserWorkspaceMemory {
    noop_user_workspace_memory_write!(create_entry(_entry: UserWorkspaceMemoryEntry));
    noop_user_workspace_memory_write!(update_entry(_key: &'a str, _update: UserWorkspaceMemoryEntryUpdate));
    noop_user_workspace_memory_write!(upsert_entry(_entry: UserWorkspaceMemoryEntry));
    noop_user_workspace_memory_write!(replace_entry(_entry: UserWorkspaceMemoryEntry));
    noop_user_workspace_memory_write!(remove_entry(_key: &'a str));
    noop_user_workspace_memory_write!(replace_document(_content: String, _base_version: ExpectedFileVersion));
}

impl MemoryWriter for NoopMemoryWriter {
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

/// Builds an Inner Model client that reuses one example's mock Provider Router.
pub fn mock_inner_model(
    router: Arc<dyn LlmProviderRouter>,
    selection: &ModelSelection,
) -> talos_error::TalosResult<Arc<InnerModelClient>> {
    Ok(Arc::new(
        InnerModelClient::builder()
            .configured_model(None)
            .agent_default_model(selection.model_ref.clone())
            .model_catalog(vec![ModelInfo::from_provider_model(
                selection.model_ref.provider.clone(),
                serde_json::from_value(serde_json::json!({"id": selection.model_ref.id})).unwrap(),
            )])
            .provider_router(router)
            .profile_store(Arc::new(talos_store::profile::MockProfileStore::default()))
            .profile_timezone("UTC".to_string())
            .build()?,
    ))
}
