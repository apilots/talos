use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};

use talos_auth::{AuthService, UserService};
use talos_config::{LlmConfig, LlmConfigService};
use talos_error::TalosResult;
use talos_gateway::Gateway;
use talos_llm_provider::LlmProvider;
use talos_session_store::{MessageIdempotencyStore, SessionEventService};
use talos_types::{resolve_model_catalog, ModelInfo, SessionService, TalosConfig};

use super::access::ResourceAccessService;
use super::lifecycle::ServerLifecycle;

#[derive(Clone, Default)]
pub(super) struct FileLockRegistry {
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl FileLockRegistry {
    pub(super) async fn lock_for(&self, key: impl Into<String>) -> Arc<Mutex<()>> {
        let key = key.into();
        let mut locks = self.locks.lock().await;
        locks
            .entry(key)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

#[derive(Clone)]
pub(super) struct HttpApiState {
    pub(super) gateway: Arc<dyn Gateway>,
    pub(super) access_service: Arc<ResourceAccessService>,
    pub(super) event_service: Arc<SessionEventService>,
    pub(super) idempotency_store: Arc<dyn MessageIdempotencyStore>,
    pub(super) base_config: TalosConfig,
    pub(super) llm_provider: Arc<dyn LlmProvider>,
    pub(super) auth_service: Arc<dyn AuthService>,
    pub(super) user_service: Arc<dyn UserService>,
    pub(super) auth_disabled: bool,
    pub(super) file_locks: FileLockRegistry,
    pub(super) models: Arc<RwLock<Vec<ModelInfo>>>,
    pub(super) llm_config_service: Arc<dyn LlmConfigService>,
    pub(super) lifecycle: Arc<ServerLifecycle>,
}

impl HttpApiState {
    #[cfg(test)]
    pub(super) fn new(
        gateway: Arc<dyn Gateway>,
        session_service: Arc<dyn SessionService>,
        event_service: Arc<SessionEventService>,
        idempotency_store: Arc<dyn MessageIdempotencyStore>,
        base_config: TalosConfig,
        llm_provider: Arc<dyn LlmProvider>,
        auth_service: Arc<dyn AuthService>,
        user_service: Arc<dyn UserService>,
        _jwt_secret: String,
        auth_disabled: bool,
        models: Vec<ModelInfo>,
    ) -> Self {
        use talos_config::{InMemoryLlmConfigStore, LlmConfigManager};
        let llm_config_service: Arc<dyn LlmConfigService> = Arc::new(LlmConfigManager::new(
            Arc::new(InMemoryLlmConfigStore::new(base_config.llm.clone())),
        ));
        Self::new_with_lifecycle(
            gateway,
            session_service,
            event_service,
            idempotency_store,
            base_config,
            llm_provider,
            auth_service,
            user_service,
            _jwt_secret,
            auth_disabled,
            models,
            llm_config_service,
            Arc::new(ServerLifecycle::new()),
        )
    }

    pub(super) fn new_with_lifecycle(
        gateway: Arc<dyn Gateway>,
        session_service: Arc<dyn SessionService>,
        event_service: Arc<SessionEventService>,
        idempotency_store: Arc<dyn MessageIdempotencyStore>,
        base_config: TalosConfig,
        llm_provider: Arc<dyn LlmProvider>,
        auth_service: Arc<dyn AuthService>,
        user_service: Arc<dyn UserService>,
        _jwt_secret: String,
        auth_disabled: bool,
        models: Vec<ModelInfo>,
        llm_config_service: Arc<dyn LlmConfigService>,
        lifecycle: Arc<ServerLifecycle>,
    ) -> Self {
        Self {
            access_service: Arc::new(ResourceAccessService::new(session_service.clone())),
            event_service,
            idempotency_store,
            gateway,
            base_config,
            llm_provider,
            auth_service,
            user_service,
            auth_disabled,
            file_locks: FileLockRegistry::default(),
            models: Arc::new(RwLock::new(models)),
            llm_config_service,
            lifecycle,
        }
    }

    /// Hot-reload the model catalog from a new LlmConfig. The LLM provider is not
    /// recreated here; API key / base URL changes take effect on next server start.
    pub(super) fn reload_llm(&self, new_config: &LlmConfig) -> TalosResult<()> {
        let new_catalog = resolve_model_catalog(new_config);
        *self.models.try_write().map_err(|_| {
            talos_error::TalosError::ConfigError("models lock poisoned".to_string())
        })? = new_catalog;
        Ok(())
    }
}
