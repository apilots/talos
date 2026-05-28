use std::sync::Arc;

use talos_application_service::{
    AccountService, ConfigService, EventService, MemoryService, TalosApplicationService,
    WorkspaceDocService,
};
#[cfg(test)]
use talos_auth::CloudSessionCache;
use talos_auth::{AuthService, CloudSessionCacheManager};
#[cfg(test)]
use talos_session_store::TimelineEventService;
use talos_types::TalosConfig;

use super::lifecycle::ServerLifecycle;

#[derive(Clone)]
pub(super) struct HttpApiState {
    pub(super) app: Arc<TalosApplicationService>,
    #[cfg(test)]
    pub(super) event_service: Arc<TimelineEventService>,
    pub(super) events: Arc<EventService>,
    pub(super) config: Arc<ConfigService>,
    pub(super) memory: Arc<MemoryService>,
    pub(super) workspace_docs: Arc<WorkspaceDocService>,
    pub(super) account: Arc<AccountService>,
    pub(super) base_config: TalosConfig,
    pub(super) auth_service: Arc<dyn AuthService>,
    pub(super) cloud_session_cache: CloudSessionCacheManager,
    pub(super) auth_disabled: bool,
    pub(super) lifecycle: Arc<ServerLifecycle>,
}

impl HttpApiState {
    pub(super) fn from_application_service(
        app: Arc<TalosApplicationService>,
        #[cfg(test)] event_service: Arc<TimelineEventService>,
        base_config: TalosConfig,
        auth_service: Arc<dyn AuthService>,
        auth_disabled: bool,
        lifecycle: Arc<ServerLifecycle>,
    ) -> Self {
        Self {
            app: app.clone(),
            #[cfg(test)]
            event_service,
            events: Arc::new(app.events.clone()),
            memory: Arc::new(app.memory.clone()),
            workspace_docs: Arc::new(app.workspace_docs.clone()),
            config: Arc::new(app.config.clone()),
            account: Arc::new(app.account.clone()),
            base_config,
            auth_service,
            cloud_session_cache: CloudSessionCacheManager::for_talos_home(
                talos_config::TalosPaths::resolve().talos_home,
            ),
            auth_disabled,
            lifecycle,
        }
    }

    #[cfg(test)]
    pub(super) fn with_cloud_session_cache(mut self, cache: CloudSessionCache) -> Self {
        self.cloud_session_cache = CloudSessionCacheManager::new(cache);
        self
    }
}
