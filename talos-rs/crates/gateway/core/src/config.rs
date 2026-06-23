use talos_error::TalosResult;
use talos_service::Actor;
use talos_types::{
    ConfigDeleteRequest, ConfigDeleteResponse, ConfigGetRequest, ConfigGetResponse,
    ConfigReloadRequest, ConfigReloadResponse, ConfigUpdateRequest, ConfigUpdateResponse,
};

use crate::service_gateway::ServiceGateway;

impl ServiceGateway {
    pub(crate) async fn get_config(
        &self,
        _actor: &Actor,
        request: ConfigGetRequest,
    ) -> TalosResult<ConfigGetResponse> {
        self.service.config.get_config(request)
    }

    pub(crate) async fn update_config(
        &self,
        _actor: &Actor,
        request: ConfigUpdateRequest,
    ) -> TalosResult<ConfigUpdateResponse> {
        self.service.config.update_config(request)
    }

    pub(crate) async fn delete_config(
        &self,
        _actor: &Actor,
        request: ConfigDeleteRequest,
    ) -> TalosResult<ConfigDeleteResponse> {
        self.service.config.delete_config(request)
    }

    pub(crate) async fn reload_config(
        &self,
        _actor: &Actor,
        request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse> {
        self.service.config.reload_config(request)
    }
}
