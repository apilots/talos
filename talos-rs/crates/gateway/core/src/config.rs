use talos_application_service::Actor;
use talos_error::TalosResult;
use talos_types::{
    ConfigDeleteRequest, ConfigDeleteResponse, ConfigGetRequest, ConfigGetResponse,
    ConfigReloadRequest, ConfigReloadResponse, ConfigUpdateRequest, ConfigUpdateResponse,
};

use crate::application_service_gateway::ApplicationServiceGateway;

impl ApplicationServiceGateway {
    pub(crate) async fn get_config(
        &self,
        _actor: &Actor,
        request: ConfigGetRequest,
    ) -> TalosResult<ConfigGetResponse> {
        self.application_service.config.get_config(request)
    }

    pub(crate) async fn update_config(
        &self,
        _actor: &Actor,
        request: ConfigUpdateRequest,
    ) -> TalosResult<ConfigUpdateResponse> {
        self.application_service.config.update_config(request)
    }

    pub(crate) async fn delete_config(
        &self,
        _actor: &Actor,
        request: ConfigDeleteRequest,
    ) -> TalosResult<ConfigDeleteResponse> {
        self.application_service.config.delete_config(request)
    }

    pub(crate) async fn reload_config(
        &self,
        _actor: &Actor,
        request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse> {
        self.application_service.config.reload_config(request)
    }
}
