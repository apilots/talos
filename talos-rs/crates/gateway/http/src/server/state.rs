use std::sync::Arc;

use talos_application_service::TalosApplicationService;
use talos_auth::AuthService;
#[cfg(test)]
use talos_auth::CloudSessionCache;
use talos_gateway::{ApplicationServiceGateway, Gateway};
#[cfg(test)]
use talos_service::TimelineEventService;
use talos_types::TalosConfig;

use super::lifecycle::ServerLifecycle;

#[derive(Clone)]
pub(super) struct HttpApiState {
    pub(super) app: Arc<TalosApplicationService>,
    pub(super) gateway: Arc<dyn Gateway>,
    #[cfg(test)]
    pub(super) event_service: Arc<TimelineEventService>,
    pub(super) base_config: TalosConfig,
    pub(super) auth_service: Arc<dyn AuthService>,
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
        let gateway: Arc<dyn Gateway> = Arc::new(ApplicationServiceGateway::new(app.clone()));
        Self {
            app: app.clone(),
            gateway,
            #[cfg(test)]
            event_service,
            base_config,
            auth_service,
            auth_disabled,
            lifecycle,
        }
    }

    #[cfg(test)]
    pub(super) fn with_cloud_session_cache(self, cache: CloudSessionCache) -> Self {
        self.app
            .account
            .set_cloud_session_cache(cache)
            .expect("cloud session cache should configure");
        self
    }
}
