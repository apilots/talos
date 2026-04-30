use std::sync::Arc;

use async_trait::async_trait;
use talos_app_protocol::requests::{ActorCredential, ActorCredentials};
use talos_application_service::Actor;
use talos_auth::{AuthService, RequestIdentity};
use talos_error::TalosError;

use crate::application::ProtocolResult;

#[async_trait]
pub trait ActorCredentialValidator: Send + Sync {
    async fn validate(&self, credentials: &ActorCredentials) -> ProtocolResult<Actor>;
}

#[derive(Debug, Clone)]
pub struct EnvLaunchTokenValidator {
    token: String,
    user_id: String,
}

impl EnvLaunchTokenValidator {
    pub fn from_env() -> ProtocolResult<Self> {
        let token = std::env::var("TALOS_LOCAL_LAUNCH_TOKEN").map_err(|_| {
            talos_app_protocol::errors::AppServerErrorBody::from_talos_error(TalosError::AuthError(
                "TALOS_LOCAL_LAUNCH_TOKEN is not configured".to_string(),
            ))
        })?;
        Ok(Self::new(token, "local-desktop"))
    }

    pub fn new(token: impl Into<String>, user_id: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            user_id: user_id.into(),
        }
    }
}

#[async_trait]
impl ActorCredentialValidator for EnvLaunchTokenValidator {
    async fn validate(&self, credentials: &ActorCredentials) -> ProtocolResult<Actor> {
        match &credentials.credential {
            ActorCredential::LocalLaunchToken { token } if token == &self.token => {
                Ok(Actor::full(self.user_id.clone()))
            }
            ActorCredential::LocalLaunchToken { .. } => {
                Err(unauthorized("invalid local launch token"))
            }
            ActorCredential::ExistingSessionToken { .. } => Err(unauthorized(
                "existing session token is not supported by this validator",
            )),
        }
    }
}

#[derive(Clone)]
pub struct AuthServiceCredentialValidator {
    auth_service: Arc<dyn AuthService>,
    local_launch_token: Option<String>,
}

impl AuthServiceCredentialValidator {
    pub fn new(auth_service: Arc<dyn AuthService>, local_launch_token: Option<String>) -> Self {
        Self {
            auth_service,
            local_launch_token,
        }
    }
}

#[async_trait]
impl ActorCredentialValidator for AuthServiceCredentialValidator {
    async fn validate(&self, credentials: &ActorCredentials) -> ProtocolResult<Actor> {
        match &credentials.credential {
            ActorCredential::LocalLaunchToken { token } => {
                let expected = self
                    .local_launch_token
                    .as_deref()
                    .ok_or_else(|| unauthorized("TALOS_LOCAL_LAUNCH_TOKEN is not configured"))?;
                if token != expected {
                    return Err(unauthorized("invalid local launch token"));
                }
                Ok(Actor::full("local-desktop"))
            }
            ActorCredential::ExistingSessionToken { token } => {
                let identity =
                    self.auth_service.verify_access_token(token).await.map_err(
                        talos_app_protocol::errors::AppServerErrorBody::from_talos_error,
                    )?;
                Ok(actor_from_identity(&identity))
            }
        }
    }
}

fn actor_from_identity(identity: &RequestIdentity) -> Actor {
    if identity.has_full_access() {
        Actor::full(identity.user_id.clone())
    } else {
        Actor::regular(identity.user_id.clone())
    }
}

fn unauthorized(message: impl Into<String>) -> talos_app_protocol::errors::AppServerErrorBody {
    talos_app_protocol::errors::AppServerErrorBody::from_talos_error(TalosError::AuthError(
        message.into(),
    ))
}
