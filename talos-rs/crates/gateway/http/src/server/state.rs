use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};

use talos_auth::{AuthService, UserService};
use talos_gateway::Gateway;
use talos_llm_provider::LlmProvider;
use talos_types::talos::TalosMessageResponse;
use talos_types::{ModelInfo, SessionService, TalosConfig};

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
    pub(super) base_config: TalosConfig,
    pub(super) llm_provider: Arc<dyn LlmProvider>,
    pub(super) auth_service: Arc<dyn AuthService>,
    pub(super) user_service: Arc<dyn UserService>,
    pub(super) static_token: Option<String>,
    pub(super) auth_disabled: bool,
    pub(super) idempotency_cache: Arc<RwLock<HashMap<String, TalosMessageResponse>>>,
    pub(super) file_locks: FileLockRegistry,
    pub(super) models: Arc<Vec<ModelInfo>>,
    pub(super) lifecycle: Arc<ServerLifecycle>,
}

impl HttpApiState {
    #[cfg(test)]
    pub(super) fn new(
        gateway: Arc<dyn Gateway>,
        session_service: Arc<dyn SessionService>,
        base_config: TalosConfig,
        llm_provider: Arc<dyn LlmProvider>,
        auth_service: Arc<dyn AuthService>,
        user_service: Arc<dyn UserService>,
        _jwt_secret: String,
        static_token: Option<String>,
        auth_disabled: bool,
        models: Vec<ModelInfo>,
    ) -> Self {
        Self::new_with_lifecycle(
            gateway,
            session_service,
            base_config,
            llm_provider,
            auth_service,
            user_service,
            _jwt_secret,
            static_token,
            auth_disabled,
            models,
            Arc::new(ServerLifecycle::new()),
        )
    }

    pub(super) fn new_with_lifecycle(
        gateway: Arc<dyn Gateway>,
        session_service: Arc<dyn SessionService>,
        base_config: TalosConfig,
        llm_provider: Arc<dyn LlmProvider>,
        auth_service: Arc<dyn AuthService>,
        user_service: Arc<dyn UserService>,
        _jwt_secret: String,
        static_token: Option<String>,
        auth_disabled: bool,
        models: Vec<ModelInfo>,
        lifecycle: Arc<ServerLifecycle>,
    ) -> Self {
        Self {
            access_service: Arc::new(ResourceAccessService::new(session_service.clone())),
            gateway,
            base_config,
            llm_provider,
            auth_service,
            user_service,
            static_token,
            auth_disabled,
            idempotency_cache: Arc::new(RwLock::new(HashMap::new())),
            file_locks: FileLockRegistry::default(),
            models: Arc::new(models),
            lifecycle,
        }
    }
}
