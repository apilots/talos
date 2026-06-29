use std::sync::Arc;

use async_trait::async_trait;
use talos_app_protocol::requests::{ActorCredential, ActorCredentials};
use talos_auth::{AuthService, CloudSessionCache, RequestIdentity};
use talos_service::{AccountService, Actor};

use crate::application::ProtocolResult;

#[async_trait]
pub trait ActorCredentialValidator: Send + Sync {
    async fn validate(&self, credentials: &ActorCredentials) -> ProtocolResult<Actor>;
}

#[derive(Clone)]
pub struct AuthServiceCredentialValidator {
    auth_service: Arc<dyn AuthService>,
    account: AccountService,
}

impl AuthServiceCredentialValidator {
    pub fn new(auth_service: Arc<dyn AuthService>, account: AccountService) -> Self {
        Self {
            auth_service,
            account,
        }
    }

    pub fn with_cloud_session_cache(self, cache: CloudSessionCache) -> Self {
        self.account
            .set_cloud_session_cache(cache)
            .expect("cloud session cache should configure");
        self
    }

    pub fn with_cloud_session_touch_interval_ms(self, interval_ms: i64) -> Self {
        self.account
            .set_cloud_session_touch_interval_ms(interval_ms)
            .expect("cloud session touch interval should configure");
        self
    }
}

#[async_trait]
impl ActorCredentialValidator for AuthServiceCredentialValidator {
    async fn validate(&self, credentials: &ActorCredentials) -> ProtocolResult<Actor> {
        match &credentials.credential {
            ActorCredential::ExistingSessionToken { token } => {
                let identity = match self.auth_service.verify_access_token(token).await {
                    Ok(identity) => identity,
                    Err(error) => {
                        let _ = self
                            .account
                            .clear_rejected_access_token(token, CloudSessionCache::now_ms());
                        return Err(
                            talos_app_protocol::errors::AppServerErrorBody::from_talos_error(error),
                        );
                    }
                };
                let _ = self
                    .account
                    .record_verified_access_token_use(token, CloudSessionCache::now_ms());
                Ok(actor_from_identity(&identity))
            }
        }
    }
}

fn actor_from_identity(identity: &RequestIdentity) -> Actor {
    let actor = Actor::regular(identity.user_id.clone())
        .with_username(identity.username.clone())
        .with_role(identity.role);
    match identity.access_token.clone() {
        Some(token) => actor.with_access_token(token),
        None => actor,
    }
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
