use std::sync::Arc;

use talos_agent::InnerModelClient;
use talos_llm_provider::LlmProviderRouter;
use talos_memory::{MemoryScope, MemoryWriteBatch, MemoryWriter};
use talos_types::{ModelInfo, ModelSelection};

pub struct NoopMemoryWriter;

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
