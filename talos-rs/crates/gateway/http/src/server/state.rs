use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use talos_auth::JwtAuth;
use talos_gateway::Gateway;
use talos_types::talos::TalosMessageResponse;
use talos_types::{ModelInfo, SessionService};

use super::lifecycle::ServerLifecycle;

#[derive(Clone)]
pub(super) struct HttpApiState {
    pub(super) gateway: Arc<dyn Gateway>,
    pub(super) session_service: Arc<dyn SessionService>,
    pub(super) auth: Arc<JwtAuth>,
    pub(super) static_token: Option<String>,
    pub(super) auth_disabled: bool,
    pub(super) idempotency_cache: Arc<RwLock<HashMap<String, TalosMessageResponse>>>,
    pub(super) models: Arc<Vec<ModelInfo>>,
    pub(super) lifecycle: Arc<ServerLifecycle>,
}

impl HttpApiState {
    #[cfg(test)]
    pub(super) fn new(
        gateway: Arc<dyn Gateway>,
        session_service: Arc<dyn SessionService>,
        jwt_secret: String,
        static_token: Option<String>,
        auth_disabled: bool,
        models: Vec<ModelInfo>,
    ) -> Self {
        Self::new_with_lifecycle(
            gateway,
            session_service,
            jwt_secret,
            static_token,
            auth_disabled,
            models,
            Arc::new(ServerLifecycle::new()),
        )
    }

    pub(super) fn new_with_lifecycle(
        gateway: Arc<dyn Gateway>,
        session_service: Arc<dyn SessionService>,
        jwt_secret: String,
        static_token: Option<String>,
        auth_disabled: bool,
        models: Vec<ModelInfo>,
        lifecycle: Arc<ServerLifecycle>,
    ) -> Self {
        Self {
            gateway,
            session_service,
            auth: Arc::new(JwtAuth::new(&jwt_secret)),
            static_token,
            auth_disabled,
            idempotency_cache: Arc::new(RwLock::new(HashMap::new())),
            models: Arc::new(models),
            lifecycle,
        }
    }
}
