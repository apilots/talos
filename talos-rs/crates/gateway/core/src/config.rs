use talos_auth::UserRole;
use talos_error::{TalosError, TalosResult};
use talos_service::Actor;
use talos_types::{
    ConfigDeleteRequest, ConfigDeleteResponse, ConfigGetRequest, ConfigGetResponse,
    ConfigReloadRequest, ConfigReloadResponse, ConfigUpdateRequest, ConfigUpdateResponse,
};

use crate::gateway_service::ServiceGateway;

impl ServiceGateway {
    pub(crate) async fn get_config(
        &self,
        actor: &Actor,
        request: ConfigGetRequest,
    ) -> TalosResult<ConfigGetResponse> {
        require_config_admin(actor)?;
        self.service.config.get_config(request)
    }

    pub(crate) async fn update_config(
        &self,
        actor: &Actor,
        request: ConfigUpdateRequest,
    ) -> TalosResult<ConfigUpdateResponse> {
        require_config_admin(actor)?;
        self.service.config.update_config(request)
    }

    pub(crate) async fn delete_config(
        &self,
        actor: &Actor,
        request: ConfigDeleteRequest,
    ) -> TalosResult<ConfigDeleteResponse> {
        require_config_admin(actor)?;
        self.service.config.delete_config(request)
    }

    pub(crate) async fn reload_config(
        &self,
        actor: &Actor,
        request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse> {
        require_config_admin(actor)?;
        self.service.config.reload_config(request)
    }
}

fn require_config_admin(actor: &Actor) -> TalosResult<()> {
    if actor.auth_disabled || actor.role == UserRole::Admin {
        return Ok(());
    }
    Err(TalosError::public(
        talos_error::ErrorCode::PermissionDenied,
        "configuration management requires an admin user".to_string(),
    ))
}
