use talos_application_service::Actor;
use talos_auth::{
    CloudLoginCancelRequest, CloudLoginStartRequest, CloudLoginStartResponse,
    CloudLoginStatusRequest, CloudLoginStatusResponse,
};
use talos_error::TalosResult;
use talos_types::{
    AuthLogoutRequest, AuthLogoutResponse, AuthSessionClearRequest, AuthSessionClearResponse,
    AuthSessionReadRequest, AuthSessionReadResponse, AuthSessionStoreRequest,
    AuthSessionStoreResponse, CloudLoginCancelResponse, UserCurrentRequest, UserCurrentResponse,
};

use crate::application_service_gateway::ApplicationServiceGateway;

impl ApplicationServiceGateway {
    pub(crate) async fn logout(
        &self,
        actor: &Actor,
        request: AuthLogoutRequest,
    ) -> TalosResult<AuthLogoutResponse> {
        self.application_service
            .account
            .logout_with_request(actor, request)
            .await?;
        Ok(AuthLogoutResponse { success: true })
    }

    pub(crate) async fn start_cloud_login(
        &self,
        _actor: &Actor,
        request: CloudLoginStartRequest,
    ) -> TalosResult<CloudLoginStartResponse> {
        self.application_service
            .account
            .start_cloud_login(request)
            .await
    }

    pub(crate) async fn cloud_login_status(
        &self,
        _actor: &Actor,
        request: CloudLoginStatusRequest,
    ) -> TalosResult<CloudLoginStatusResponse> {
        self.application_service
            .account
            .cloud_login_status(request)
            .await
    }

    pub(crate) async fn cancel_cloud_login(
        &self,
        _actor: &Actor,
        request: CloudLoginCancelRequest,
    ) -> TalosResult<CloudLoginCancelResponse> {
        self.application_service
            .account
            .cancel_cloud_login(request)
            .await?;
        Ok(CloudLoginCancelResponse { success: true })
    }

    pub(crate) async fn read_auth_session(
        &self,
        actor: &Actor,
        request: AuthSessionReadRequest,
    ) -> TalosResult<AuthSessionReadResponse> {
        self.application_service
            .account
            .read_auth_session(actor, request)
    }

    pub(crate) async fn store_auth_session(
        &self,
        actor: &Actor,
        request: AuthSessionStoreRequest,
    ) -> TalosResult<AuthSessionStoreResponse> {
        self.application_service
            .account
            .store_auth_session(actor, request)?;
        Ok(AuthSessionStoreResponse { success: true })
    }

    pub(crate) async fn clear_auth_session(
        &self,
        _actor: &Actor,
        request: AuthSessionClearRequest,
    ) -> TalosResult<AuthSessionClearResponse> {
        self.application_service
            .account
            .clear_auth_session(request)?;
        Ok(AuthSessionClearResponse { success: true })
    }

    pub(crate) async fn current_user(
        &self,
        actor: &Actor,
        request: UserCurrentRequest,
    ) -> TalosResult<UserCurrentResponse> {
        let user = self
            .application_service
            .account
            .current_user_with_request(actor, request)
            .await?;
        Ok(UserCurrentResponse { user })
    }
}
