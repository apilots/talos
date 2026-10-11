use talos_error::TalosResult;
use talos_service::Actor;
use talos_types::{
    ConfigDeleteRequest, ConfigDeleteResponse, ConfigGetRequest, ConfigReloadRequest,
    ConfigReloadResponse, ConfigResponse, ConfigUpdateRequest,
};

use crate::gateway_service::ServiceGateway;

impl ServiceGateway {
    pub(crate) async fn get_config(
        &self,
        actor: &Actor,
        request: ConfigGetRequest,
    ) -> TalosResult<ConfigResponse> {
        self.service.get_config(actor, request).await
    }

    pub(crate) async fn update_config(
        &self,
        actor: &Actor,
        request: ConfigUpdateRequest,
    ) -> TalosResult<ConfigResponse> {
        self.service.update_config(actor, request).await
    }

    pub(crate) async fn delete_config(
        &self,
        actor: &Actor,
        request: ConfigDeleteRequest,
    ) -> TalosResult<ConfigDeleteResponse> {
        self.service.delete_config(actor, request).await
    }

    pub(crate) async fn reload_config(
        &self,
        actor: &Actor,
        request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse> {
        self.service.reload_config(actor, request).await
    }
}
