use super::*;
/// One local authorization attempt. Dropping it closes its callback listener and SDK state.
pub struct OAuthAuthorization {
    session: AuthorizationSession,
    memory: InMemoryCredentialStore,
    listener: Option<tokio::net::TcpListener>,
    expected_issuer: Option<String>,
    require_issuer: bool,
    client_secret: Option<String>,
}
impl OAuthAuthorization {
    pub async fn start(
        url: &str,
        config: &McpOAuthConfig,
        client_secret: Option<String>,
    ) -> TalosResult<Self> {
        Self::start_with_redirect(url, config, client_secret, /*relay_uri*/ None).await
    }

    /// Uses a trusted HTTPS relay; no callback listener is opened on the Worker.
    pub async fn start_relay(
        url: &str,
        config: &McpOAuthConfig,
        client_secret: Option<String>,
        relay_uri: &str,
    ) -> TalosResult<Self> {
        let relay = url::Url::parse(relay_uri).map_err(|_| invalid_callback())?;
        if relay.scheme() != "https"
            || !relay.username().is_empty()
            || relay.password().is_some()
            || relay.query().is_some()
            || relay.fragment().is_some()
        {
            return Err(invalid_callback());
        }
        Self::start_with_redirect(url, config, client_secret, Some(relay_uri)).await
    }

    async fn start_with_redirect(
        url: &str,
        config: &McpOAuthConfig,
        client_secret: Option<String>,
        relay_uri: Option<&str>,
    ) -> TalosResult<Self> {
        let resource = endpoint(url)?;
        validate_config(config, client_secret.as_deref())?;
        if client_secret.is_some() && config.client_id.is_none() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "An OAuth client secret requires a pre-registered client ID.",
            ));
        }
        let (listener, redirect) = match relay_uri {
            Some(uri) => (None, uri.to_owned()),
            None => {
                let listener = tokio::net::TcpListener::bind((
                    std::net::Ipv4Addr::LOCALHOST,
                    config.callback_port.unwrap_or(0),
                ))
                .await
                .map_err(|error| {
                    TalosError::internal_with_source(
                        "Failed to bind the local MCP OAuth callback.",
                        error,
                    )
                })?;
                let redirect = format!(
                    "http://{}/mcp/oauth/callback",
                    listener
                        .local_addr()
                        .map_err(|error| TalosError::internal_with_source(
                            "Failed to read the OAuth callback address.",
                            error
                        ))?
                );
                (Some(listener), redirect)
            }
        };
        let mut manager = AuthorizationManager::new(url).await.map_err(auth_error)?;
        let memory = InMemoryCredentialStore::new();
        manager.set_credential_store(memory.clone());
        let resolution = manager.resolve_metadata().await.map_err(auth_error)?;
        validate_metadata(&resource, &resolution.metadata)?;
        let metadata_client = config.client_metadata_url.is_some()
            && resolution
                .metadata
                .additional_fields
                .get("client_id_metadata_document_supported")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
        let expected_issuer = resolution.metadata.issuer.clone();
        let require_issuer = resolution
            .metadata
            .additional_fields
            .get("authorization_response_iss_parameter_supported")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        manager.set_metadata(resolution.metadata);
        let mut request = AuthorizationRequest::new(redirect)
            .with_client_name("Talos")
            .with_scopes(config.scopes.clone());
        request.client_id = config.client_id.clone();
        request.client_secret = client_secret;
        request.client_metadata_url = config.client_metadata_url.clone();
        if request.client_id.is_none() && !metadata_client {
            // Keep any registration secret for refresh after restart. Session still owns PKCE and scope selection.
            let scopes = if config.scopes.is_empty() {
                manager.select_scopes(None, &[])
            } else {
                config.scopes.clone()
            };
            let registered = manager
                .register_client(
                    "Talos",
                    &request.redirect_uri,
                    &scopes.iter().map(String::as_str).collect::<Vec<_>>(),
                )
                .await
                .map_err(auth_error)?;
            request.client_id = Some(registered.client_id);
            request.client_secret = registered.client_secret;
        }
        let client_secret = request.client_secret.clone();
        let session = AuthorizationSession::new(manager, request)
            .await
            .map_err(|(_, error)| auth_error(error))?;
        Ok(Self {
            session,
            memory,
            listener,
            expected_issuer,
            require_issuer,
            client_secret,
        })
    }
    pub fn authorization_url(&self) -> &str {
        self.session.get_authorization_url()
    }
    /// The caller owns the attempt deadline and cancellation. Unrelated callbacks
    /// never consume the attempt; the SDK owns PKCE, issuer validation and exchange.
    pub async fn finish(self) -> TalosResult<OAuthCredentials> {
        let authorization_url =
            url::Url::parse(self.session.get_authorization_url()).map_err(|error| {
                TalosError::internal_with_source(
                    "The SDK returned an invalid authorization URL.",
                    error,
                )
            })?;
        let state = authorization_url
            .query_pairs()
            .find(|(name, _)| name == "state")
            .map(|(_, state)| state.into_owned())
            .filter(|state| !state.is_empty())
            .ok_or_else(|| TalosError::internal("The SDK authorization URL has no state."))?;
        loop {
            let callback = callback::receive(
                self.listener.as_ref().ok_or_else(invalid_callback)?,
                &self.session.redirect_uri,
                &state,
            )
            .await?;
            match self.session.handle_callback_url(&callback).await {
                Ok(_) => break,
                // These failures precede state consumption in the SDK. Preserve
                // the valid PKCE attempt for the actual authorization response.
                Err(
                    AuthError::AuthorizationServerMismatch { .. }
                    | AuthError::AuthorizationServerMissingIssuer { .. }
                    | AuthError::AuthorizationFailed(_),
                ) => continue,
                Err(error) => return Err(auth_error(error)),
            }
        }
        self.credentials().await
    }

    /// Rejects forged callbacks before one-time SDK exchange. InvalidRequest leaves the
    /// valid attempt intact; the caller owns exclusion, cancellation, expiry and publication.
    pub async fn complete_callback(&self, callback_url: &str) -> TalosResult<OAuthCredentials> {
        if self.validate_relay_callback(callback_url)? {
            return Err(TalosError::new(
                ErrorCode::Cancelled,
                "The MCP authorization request was declined.",
            ));
        }
        match self.session.handle_callback_url(callback_url).await {
            Ok(_) => self.credentials().await,
            Err(
                AuthError::AuthorizationServerMismatch { .. }
                | AuthError::AuthorizationServerMissingIssuer { .. }
                | AuthError::AuthorizationFailed(_),
            ) => Err(invalid_callback()),
            Err(error) => Err(auth_error(error)),
        }
    }

    fn validate_relay_callback(&self, callback: &str) -> TalosResult<bool> {
        let callback = url::Url::parse(callback).map_err(|_| invalid_callback())?;
        let authorization =
            url::Url::parse(self.authorization_url()).map_err(|_| invalid_callback())?;
        let mut bound = callback.clone();
        bound.set_query(None);
        if bound.as_str() != self.session.redirect_uri || callback.fragment().is_some() {
            return Err(invalid_callback());
        }
        let parameter = |name: &str| {
            callback
                .query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
        };
        let expected_state = authorization
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned());
        if parameter("state") != expected_state || expected_state.is_none() {
            return Err(invalid_callback());
        }
        let issuer = parameter("iss");
        if (self.require_issuer && issuer.is_none())
            || issuer
                .as_ref()
                .is_some_and(|issuer| Some(issuer) != self.expected_issuer.as_ref())
        {
            return Err(invalid_callback());
        }
        match (parameter("code"), parameter("error")) {
            (Some(code), None) if !code.is_empty() => Ok(false),
            (None, Some(error)) if !error.is_empty() => Ok(true),
            _ => Err(invalid_callback()),
        }
    }

    async fn credentials(&self) -> TalosResult<OAuthCredentials> {
        let stored = self
            .memory
            .load()
            .await
            .map_err(auth_error)?
            .ok_or_else(|| TalosError::internal("OAuth completed without storing credentials."))?;
        Ok(OAuthCredentials {
            stored,
            client_secret: self.client_secret.clone(),
            resource: authorization_resource(self.authorization_url())?,
            redirect_uri: self.session.redirect_uri.clone(),
        })
    }
}

fn invalid_callback() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "The MCP authorization callback is invalid.",
    )
}
