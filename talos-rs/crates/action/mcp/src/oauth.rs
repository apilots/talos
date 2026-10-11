use futures_util::future::BoxFuture;
use oauth2::TokenResponse;
use rmcp::transport::auth::{
    AuthClient, AuthError, AuthorizationManager, AuthorizationRequest, AuthorizationSession,
    CredentialRefreshGuard, CredentialStore, InMemoryCredentialStore, OAuthClientConfig,
    StoredCredentials,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{McpAuthenticationState, McpOAuthConfig};
mod callback;

/// Private persistence payload. SDK token representation never appears in the application protocol.
#[derive(Clone, Serialize, Deserialize)]
pub struct OAuthCredentials {
    stored: StoredCredentials,
    client_secret: Option<String>,
    redirect_uri: String,
    resource: String,
}
impl OAuthCredentials {
    pub fn state(&self) -> McpAuthenticationState {
        let Some(token) = &self.stored.token_response else {
            return McpAuthenticationState::Missing;
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if token.expires_in().is_some_and(|expires| {
            self.stored
                .token_received_at
                .is_none_or(|received| received.saturating_add(expires.as_secs()) <= now)
        }) {
            McpAuthenticationState::Expired
        } else {
            McpAuthenticationState::Authenticated
        }
    }
}

/// Application-owned persistence. Save must reject missing/revoked records instead of recreating them.
/// The refresh guard spans SDK load, network refresh and save; individual methods must not reacquire it.
pub trait OAuthCredentialStore: Send + Sync {
    fn load(&self) -> BoxFuture<'_, TalosResult<Option<OAuthCredentials>>>;
    fn save(&self, credentials: OAuthCredentials) -> BoxFuture<'_, TalosResult<()>>;
    fn clear(&self) -> BoxFuture<'_, TalosResult<()>>;
    fn refresh_guard(&self) -> BoxFuture<'_, TalosResult<Box<dyn Send>>>;
}
#[derive(Clone)]
pub struct OAuthCredentialSource {
    pub reference: String,
    pub store: Arc<dyn OAuthCredentialStore>,
}
impl PartialEq for OAuthCredentialSource {
    fn eq(&self, other: &Self) -> bool {
        self.reference == other.reference
    }
}

struct SdkStore {
    store: Arc<dyn OAuthCredentialStore>,
    failure: Mutex<Option<tokio::sync::oneshot::Sender<TalosError>>>,
}
impl SdkStore {
    fn error(&self, error: TalosError) -> AuthError {
        // The SDK only accepts a string. The existing connection worker owns
        // this first terminal cause; later cleanup failures remain observable.
        match self
            .failure
            .lock()
            .expect("OAuth failure lock poisoned")
            .take()
        {
            Some(sender) => {
                if let Err(error) = sender.send(error) {
                    talos_logging::report_error(&error);
                }
            }
            None => talos_logging::report_error(&error),
        }
        AuthError::CredentialStoreError("The MCP credential store operation failed.".into())
    }
}

pub(super) async fn store_failure(
    receiver: &mut Option<tokio::sync::oneshot::Receiver<TalosError>>,
) -> TalosError {
    if let Some(receiver) = receiver {
        if let Ok(error) = receiver.await {
            return error;
        }
    }
    *receiver = None;
    std::future::pending().await
}

pub(super) fn take_store_failure(
    receiver: &mut Option<tokio::sync::oneshot::Receiver<TalosError>>,
) -> Option<TalosError> {
    receiver.take().and_then(|mut receiver| {
        receiver.close();
        receiver.try_recv().ok()
    })
}
#[async_trait::async_trait]
impl CredentialStore for SdkStore {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        self.store
            .load()
            .await
            .map(|record| record.map(|record| record.stored))
            .map_err(|error| self.error(error))
    }
    async fn save(&self, stored: StoredCredentials) -> Result<(), AuthError> {
        let mut record = self
            .store
            .load()
            .await
            .map_err(|error| self.error(error))?
            .ok_or_else(|| {
                AuthError::CredentialStoreError("MCP credentials were removed.".into())
            })?;
        record.stored = stored;
        self.store
            .save(record)
            .await
            .map_err(|error| self.error(error))
    }
    async fn clear(&self) -> Result<(), AuthError> {
        self.store.clear().await.map_err(|error| self.error(error))
    }
    async fn acquire_refresh_guard(&self) -> Result<Option<CredentialRefreshGuard>, AuthError> {
        self.store
            .refresh_guard()
            .await
            .map(|guard| Some(CredentialRefreshGuard::new(guard)))
            .map_err(|error| self.error(error))
    }
}
pub(super) async fn client(
    url: &str,
    source: &OAuthCredentialSource,
    http: reqwest::Client,
    failure: tokio::sync::oneshot::Sender<TalosError>,
) -> TalosResult<AuthClient<reqwest::Client>> {
    let resource = endpoint(url)?;
    let record = source.store.load().await?.ok_or_else(|| {
        TalosError::new(ErrorCode::Unauthenticated, "MCP authorization is required.")
    })?;
    let mut manager = AuthorizationManager::new(url).await.map_err(auth_error)?;
    let resolution = manager.resolve_metadata().await.map_err(auth_error)?;
    validate_metadata(&resource, &resolution.metadata)?;
    manager.set_metadata(resolution.metadata);
    manager.set_credential_store(SdkStore {
        store: source.store.clone(),
        failure: Mutex::new(Some(failure)),
    });
    if !manager.initialize_from_store().await.map_err(auth_error)? {
        return Err(TalosError::new(
            ErrorCode::Unauthenticated,
            "MCP authorization is required.",
        ));
    }
    // Restore confidential client settings; SDK StoredCredentials intentionally excludes them.
    let mut config = OAuthClientConfig::new(record.stored.client_id, record.redirect_uri);
    config.client_secret = record.client_secret;
    config.scopes = record.stored.granted_scopes;
    manager.configure_client(config).map_err(auth_error)?;
    // rmcp exposes the selected resource only on its generated authorization URL.
    // The unused PKCE state is bounded to this connection and dropped with the manager.
    let current = manager
        .get_authorization_url(&[])
        .await
        .map_err(auth_error)?;
    if authorization_resource(&current)? != record.resource {
        return Err(TalosError::new(
            ErrorCode::Unauthenticated,
            "The MCP authorization resource changed. Authorize the service again.",
        ));
    }
    Ok(AuthClient::new(http, manager))
}

mod authorization;
pub use authorization::OAuthAuthorization;

fn authorization_resource(value: &str) -> TalosResult<String> {
    url::Url::parse(value)
        .ok()
        .and_then(|url| {
            url.query_pairs()
                .find(|(name, _)| name == "resource")
                .map(|(_, value)| value.into_owned())
        })
        .ok_or_else(|| TalosError::internal("The SDK authorization URL has no resource binding."))
}

fn endpoint(value: &str) -> TalosResult<url::Url> {
    let parsed = url::Url::parse(value).map_err(|error| {
        TalosError::new(
            ErrorCode::InvalidRequest,
            "The MCP OAuth endpoint is invalid.",
        )
        .with_source(error)
    })?;
    if value.len() > 8192
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
        || !(parsed.scheme() == "https" || parsed.scheme() == "http" && loopback(&parsed))
    {
        return Err(TalosError::new(ErrorCode::InvalidRequest, "MCP OAuth endpoints require HTTPS without user information or fragments; HTTP is allowed only on loopback."));
    }
    Ok(parsed)
}

fn loopback(url: &url::Url) -> bool {
    url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    })
}

fn validate_metadata(
    resource: &url::Url,
    metadata: &rmcp::transport::auth::AuthorizationMetadata,
) -> TalosResult<()> {
    for value in [
        Some(metadata.authorization_endpoint.as_str()),
        Some(metadata.token_endpoint.as_str()),
        metadata.registration_endpoint.as_deref(),
        metadata.issuer.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        let target = endpoint(value)?;
        if target.scheme() == "http" && !(resource.scheme() == "http" && loopback(resource)) {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Remote MCP OAuth metadata must not downgrade authorization to HTTP.",
            ));
        }
    }
    Ok(())
}

fn validate_config(config: &McpOAuthConfig, secret: Option<&str>) -> TalosResult<()> {
    let bytes = serde_json::to_vec(config).map_err(|error| {
        TalosError::internal_with_source("Failed to encode MCP OAuth settings.", error)
    })?;
    if bytes.len() > 16 * 1024
        || secret.is_some_and(|secret| secret.is_empty() || secret.len() > 64 * 1024)
        || config
            .client_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty())
        || config.scopes.iter().any(|scope| {
            scope.is_empty()
                || !scope
                    .bytes()
                    .all(|byte| matches!(byte, 0x21 | 0x23..=0x5b | 0x5d..=0x7e))
        })
    {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "MCP OAuth settings contain invalid values or exceed their size limit.",
        ));
    }
    if let Some(value) = &config.client_metadata_url {
        let url = endpoint(value)?;
        if url.scheme() != "https" || url.path() == "/" {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "OAuth client metadata requires an HTTPS document URL.",
            ));
        }
    }
    Ok(())
}

fn auth_error(error: AuthError) -> TalosError {
    let code = match &error {
        AuthError::AuthorizationRequired | AuthError::TokenRefreshRejected(_) => {
            ErrorCode::Unauthenticated
        }
        AuthError::CredentialStoreError(_) => ErrorCode::InternalError,
        _ => ErrorCode::ToolExecutionFailed,
    };
    TalosError::from_source(
        code,
        "MCP authorization failed. Check the service authentication settings.",
        PrivateAuthError(error),
    )
}
// Provider errors can contain response bodies or callback URLs. Keep their typed source private.
struct PrivateAuthError(AuthError);
impl std::fmt::Display for PrivateAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match &self.0 {
            AuthError::AuthorizationRequired => "OAuth authorization required",
            AuthError::CredentialStoreError(_) => "OAuth credential storage failed",
            AuthError::TokenRefreshRejected(_) => "OAuth refresh rejected",
            _ => "OAuth protocol operation failed",
        })
    }
}
impl std::fmt::Debug for PrivateAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl std::error::Error for PrivateAuthError {}
