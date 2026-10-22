use std::sync::{Arc, RwLock};

use talos_auth::{
    AuthService, CloudLoginCancelRequest, CloudLoginPersistence, CloudLoginService,
    CloudLoginStartRequest, CloudLoginStartResponse, CloudLoginStatusRequest,
    CloudLoginStatusResponse, CloudSessionCache, CloudSessionCacheEntry, CloudSessionCacheManager,
    RequestIdentity, UserDto, UserService,
};
use talos_config::TalosPaths;
use talos_error::{TalosError, TalosResult};
use talos_types::{
    AuthLogoutRequest, AuthSessionClearRequest, AuthSessionReadRequest, AuthSessionReadResponse,
    AuthSessionSnapshot, AuthSessionUserSnapshot, UserCurrentRequest,
};

use crate::access::Actor;

#[derive(Clone)]
pub struct AccountService {
    auth_service: Arc<dyn AuthService>,
    user_service: Arc<dyn UserService>,
    cloud_session: Arc<RwLock<CloudSessionServices>>,
}

struct CloudSessionServices {
    cloud_api_base_url: String,
    session_cache: CloudSessionCacheManager,
    login: Arc<CloudLoginService>,
}

impl AccountService {
    pub fn new(
        auth_service: Arc<dyn AuthService>,
        user_service: Arc<dyn UserService>,
        cloud_api_base_url: impl Into<String>,
    ) -> Self {
        let cloud_api_base_url = cloud_api_base_url.into();
        let session_cache =
            CloudSessionCacheManager::for_talos_home(TalosPaths::resolve().talos_home);
        let login = Arc::new(CloudLoginService::new(
            cloud_api_base_url.clone(),
            session_cache.clone(),
        ));
        Self {
            auth_service,
            user_service,
            cloud_session: Arc::new(RwLock::new(CloudSessionServices {
                cloud_api_base_url,
                session_cache,
                login,
            })),
        }
    }

    pub fn set_cloud_session_cache(&self, cache: CloudSessionCache) -> TalosResult<()> {
        let mut cloud_session = self.cloud_session_write()?;
        cloud_session.session_cache = CloudSessionCacheManager::new(cache);
        cloud_session.login = Arc::new(CloudLoginService::new(
            cloud_session.cloud_api_base_url.clone(),
            cloud_session.session_cache.clone(),
        ));
        Ok(())
    }

    pub fn set_cloud_session_touch_interval_ms(&self, interval_ms: i64) -> TalosResult<()> {
        let mut cloud_session = self.cloud_session_write()?;
        cloud_session.session_cache = cloud_session
            .session_cache
            .clone()
            .with_touch_interval_ms(interval_ms);
        cloud_session.login = Arc::new(CloudLoginService::new(
            cloud_session.cloud_api_base_url.clone(),
            cloud_session.session_cache.clone(),
        ));
        Ok(())
    }

    pub fn set_cloud_api_base_url(&self, api_base_url: impl Into<String>) -> TalosResult<()> {
        let mut cloud_session = self.cloud_session_write()?;
        cloud_session.cloud_api_base_url = api_base_url.into();
        cloud_session.login = Arc::new(CloudLoginService::new(
            cloud_session.cloud_api_base_url.clone(),
            cloud_session.session_cache.clone(),
        ));
        Ok(())
    }

    fn cloud_session_cache(&self) -> TalosResult<CloudSessionCacheManager> {
        Ok(self.cloud_session_read()?.session_cache.clone())
    }

    pub fn record_verified_access_token_use(&self, token: &str, now_ms: i64) -> TalosResult<()> {
        self.cloud_session_cache()?
            .record_verified_token_use(token, now_ms)
            .map(|_| ())
    }

    pub fn clear_rejected_access_token(&self, token: &str, now_ms: i64) -> TalosResult<()> {
        self.auth_service.invalidate_access_token(token);
        self.cloud_session_cache()?
            .clear_rejected_token(token, now_ms)
    }

    pub async fn logout(&self, actor: &RequestIdentity) -> TalosResult<()> {
        self.auth_service.logout(actor).await
    }

    pub async fn current_user(&self, actor: &RequestIdentity) -> TalosResult<UserDto> {
        self.user_service.get_current_user(actor).await
    }

    pub async fn logout_with_request(
        &self,
        actor: &Actor,
        _request: AuthLogoutRequest,
    ) -> TalosResult<()> {
        self.logout(&request_identity(actor)).await?;
        self.cloud_session_cache()?.clear()?;
        Ok(())
    }

    pub async fn current_user_with_request(
        &self,
        actor: &Actor,
        _request: UserCurrentRequest,
    ) -> TalosResult<UserDto> {
        self.current_user(&request_identity(actor)).await
    }

    pub async fn start_cloud_login(
        &self,
        request: CloudLoginStartRequest,
        persistence: CloudLoginPersistence,
    ) -> TalosResult<CloudLoginStartResponse> {
        self.cloud_login()?.start(request, persistence).await
    }

    pub async fn cloud_login_status(
        &self,
        request: CloudLoginStatusRequest,
    ) -> TalosResult<CloudLoginStatusResponse> {
        self.cloud_login()?
            .status(request, self.auth_service.as_ref())
            .await
    }

    pub async fn cancel_cloud_login(&self, request: CloudLoginCancelRequest) -> TalosResult<()> {
        self.cloud_login()?.cancel(request).await
    }

    pub fn read_auth_session(
        &self,
        actor: &Actor,
        _request: AuthSessionReadRequest,
    ) -> TalosResult<AuthSessionReadResponse> {
        let session = self
            .cloud_session_cache()?
            .read_candidate(CloudSessionCache::now_ms())?
            .filter(|entry| match actor.access_token.as_deref() {
                Some(token) => entry.access_token == token,
                None => true,
            })
            .map(auth_session_snapshot_from_cache_entry);
        Ok(AuthSessionReadResponse { session })
    }

    pub fn clear_auth_session(&self, _request: AuthSessionClearRequest) -> TalosResult<()> {
        let cache = self.cloud_session_cache()?;
        if let Some(entry) = cache.read_candidate(CloudSessionCache::now_ms())? {
            self.auth_service
                .invalidate_access_token(&entry.access_token);
        }
        cache.clear()
    }

    fn cloud_login(&self) -> TalosResult<Arc<CloudLoginService>> {
        Ok(self.cloud_session_read()?.login.clone())
    }

    fn cloud_session_read(
        &self,
    ) -> TalosResult<std::sync::RwLockReadGuard<'_, CloudSessionServices>> {
        self.cloud_session
            .read()
            .map_err(|_| TalosError::internal("cloud session service lock poisoned"))
    }

    fn cloud_session_write(
        &self,
    ) -> TalosResult<std::sync::RwLockWriteGuard<'_, CloudSessionServices>> {
        self.cloud_session
            .write()
            .map_err(|_| TalosError::internal("cloud session service lock poisoned"))
    }
}

fn request_identity(actor: &Actor) -> RequestIdentity {
    RequestIdentity {
        user_id: actor.user_id.clone(),
        username: actor
            .username
            .clone()
            .or_else(|| Some(actor.user_id.clone())),
        role: actor.role,
        auth_disabled: false,
        access_token: actor.access_token.clone(),
    }
}

fn auth_session_snapshot_from_cache_entry(entry: CloudSessionCacheEntry) -> AuthSessionSnapshot {
    AuthSessionSnapshot {
        schema_version: entry.schema_version,
        cloud_api_base_url: entry.cloud_api_base_url,
        access_token: entry.access_token,
        token_type: entry.token_type,
        expires_at_ms: entry.expires_at_ms.map(|value| value.to_string()),
        last_used_at_ms: entry.last_used_at_ms.to_string(),
        user: AuthSessionUserSnapshot {
            user_id: entry.user.user_id,
            username: entry.user.username,
            email: entry.user.email,
            role: entry.user.role,
            r#type: entry.user.r#type,
        },
    }
}
