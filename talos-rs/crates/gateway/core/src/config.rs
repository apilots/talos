use talos_auth::UserRole;
use talos_config::DeploymentMode;
use talos_error::{TalosError, TalosResult};
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
        require_config_access(actor, self.service.mode())?;
        self.service.config.get_config(request)
    }

    pub(crate) async fn update_config(
        &self,
        actor: &Actor,
        request: ConfigUpdateRequest,
    ) -> TalosResult<ConfigResponse> {
        require_config_access(actor, self.service.mode())?;
        self.service.config.update_config(request).await
    }

    pub(crate) async fn delete_config(
        &self,
        actor: &Actor,
        request: ConfigDeleteRequest,
    ) -> TalosResult<ConfigDeleteResponse> {
        require_config_access(actor, self.service.mode())?;
        self.service.config.delete_config(request)
    }

    pub(crate) async fn reload_config(
        &self,
        actor: &Actor,
        request: ConfigReloadRequest,
    ) -> TalosResult<ConfigReloadResponse> {
        require_config_access(actor, self.service.mode())?;
        self.service.config.reload_config(request).await
    }
}

fn require_config_access(actor: &Actor, mode: DeploymentMode) -> TalosResult<()> {
    if actor.auth_disabled || mode == DeploymentMode::Local || actor.role == UserRole::Admin {
        return Ok(());
    }
    Err(TalosError::new(
        talos_error::ErrorCode::PermissionDenied,
        "configuration management requires an admin user".to_string(),
    ))
}
