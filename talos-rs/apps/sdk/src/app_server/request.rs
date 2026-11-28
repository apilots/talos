//! SDK request serialization and response correlation; transport policy stays private.

use super::credentials::actor_credentials_for_method;
use super::{AppServerClient, APP_SERVER_PLUGIN_REQUEST_TIMEOUT, APP_SERVER_REQUEST_TIMEOUT};
use serde::de::DeserializeOwned;
use serde_json::Value;
use talos_app_protocol::jsonrpc::{JsonRpcRequest, RequestId};
use talos_app_protocol::requests::ActorCredentials;
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::io::AsyncWriteExt;
use tokio::sync::oneshot;

impl AppServerClient {
    pub(super) async fn request_typed<P, T>(&mut self, method: &str, params: P) -> TalosResult<T>
    where
        P: serde::Serialize,
        T: DeserializeOwned,
    {
        let value = self.request_value(method, params).await?;
        serde_json::from_value(value).map_err(|error| {
            TalosError::internal_with_source(
                "App-server communication failed; technical details were withheld.",
                error,
            )
        })
    }

    pub(super) async fn request_value<P>(&mut self, method: &str, params: P) -> TalosResult<Value>
    where
        P: serde::Serialize,
    {
        if let Some(http) = &self.product_http {
            return http.request(method, params).await;
        }
        let id = self.next_request_id();
        let params = serde_json::to_value(params).map_err(|error| {
            TalosError::internal_with_source(
                "App-server communication failed; technical details were withheld.",
                error,
            )
        })?;
        let mut request = JsonRpcRequest::new(RequestId::Number(id), method, Some(params));
        request.actor_credentials = self.actor_credentials_for_request(method);
        request.computer_id = self.computer_id.clone();
        let (tx, rx) = oneshot::channel();
        self.pending_responses.lock().await.insert(id, tx);
        if let Err(error) = self.write_json(&request).await {
            self.pending_responses.lock().await.remove(&id);
            return Err(error);
        }
        let timeout = match method {
            "plugin/install" | "plugin/update" => APP_SERVER_PLUGIN_REQUEST_TIMEOUT,
            _ => APP_SERVER_REQUEST_TIMEOUT,
        };
        let response = match tokio::time::timeout(timeout, rx).await {
            Ok(response) => response,
            Err(_) => {
                self.pending_responses.lock().await.remove(&id);
                return Err(TalosError::internal("app-server request timeout"));
            }
        };

        let result = response.map_err(|_| {
            TalosError::internal(
                "app-server response channel closed for request; technical details were withheld.",
            )
            .context(format!(
                "app-server response channel closed for request {id}"
            ))
        })?;
        match result {
            Err(error)
                if error.is_code(ErrorCode::Unauthenticated)
                    && self.use_cloud_session_cache
                    && self.actor_credentials.is_some()
                    && method != "auth/session/clear" =>
            {
                self.actor_credentials = None;
                Err(error.context("cached cloud session was rejected; sign in again"))
            }
            other => other,
        }
    }

    fn actor_credentials_for_request(&self, method: &str) -> Option<ActorCredentials> {
        actor_credentials_for_method(method, &self.actor_credentials)
    }

    fn next_request_id(&mut self) -> i32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    pub(super) async fn write_json<T: serde::Serialize>(&mut self, value: &T) -> TalosResult<()> {
        let line = serde_json::to_string(value).map_err(|error| {
            TalosError::internal_with_source(
                "App-server communication failed; technical details were withheld.",
                error,
            )
        })?;
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| TalosError::internal("app-server client is shut down"))?;
        stdin.write_all(line.as_bytes()).await.map_err(|error| {
            TalosError::internal_with_source(
                "App-server communication failed; technical details were withheld.",
                error,
            )
        })?;
        stdin.write_all(b"\n").await.map_err(|error| {
            TalosError::internal_with_source(
                "App-server communication failed; technical details were withheld.",
                error,
            )
        })?;
        stdin.flush().await.map_err(|error| {
            TalosError::internal_with_source(
                "App-server communication failed; technical details were withheld.",
                error,
            )
        })
    }
}

impl AppServerClient {
    /// Resolve an existing synchronized file source through the same account
    /// Server. The source identity is the original stable Host ID, not a Sync connection.
    pub fn connect_source_product_http(
        endpoint: &str,
        access_token: String,
        source_peer: String,
        trusted_ca_pem: Option<&[u8]>,
    ) -> TalosResult<Self> {
        let mut client =
            Self::connect_product_http(endpoint, access_token, "embedded".into(), trusted_ca_pem)?;
        client
            .product_http
            .as_mut()
            .expect("Product HTTP transport")
            .scope = super::ProductHttpScope::SourcePeer(source_peer);
        Ok(client)
    }

    /// Bounded source preview over the existing product content API.
    pub async fn read_product_file_range(
        &self,
        file_id: &str,
        range: std::ops::Range<u64>,
    ) -> TalosResult<crate::FileDownloadResponse> {
        use base64::Engine;
        let http = self.product_http.as_ref().ok_or_else(|| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "A product HTTP scope is required.",
            )
        })?;
        if range.end <= range.start
            || range.end - range.start > u64::from(talos_types::FILE_PREVIEW_MAX_READ_BYTES)
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "File preview range is invalid.",
            ));
        }
        let mut url = http.origin.join("v1/files/").map_err(http_error)?;
        url.path_segments_mut()
            .map_err(|_| TalosError::internal("Invalid product origin."))?
            .pop_if_empty()
            .push(file_id)
            .push("content");
        url.query_pairs_mut()
            .append_pair("offset", &range.start.to_string())
            .append_pair("limit", &(range.end - range.start).to_string());
        let response = http
            .send_authorized(
                http.authorized(http.client.get(url).timeout(APP_SERVER_REQUEST_TIMEOUT)),
            )
            .await?;
        let body =
            super::response::decode_api_response(response.json().await.map_err(http_error)?)?;
        let content: talos_types::FileContentResponse =
            serde_json::from_value(body).map_err(http_error)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(content.data_base64)
            .map_err(http_error)?;
        if bytes.len() as u64 > range.end - range.start {
            return Err(TalosError::internal(
                "File source exceeded the requested range.",
            ));
        }
        Ok(crate::FileDownloadResponse {
            file: content.file,
            data: bytes.into(),
        })
    }

    /// Select a per-request product scope for the existing stdio transport.
    pub fn select_computer(&mut self, computer_id: Option<String>) {
        self.computer_id = computer_id;
    }

    /// Connect to one product through the account's Talos Server. The Server
    /// authenticates the target and routes personal products through its Relay.
    /// Local products continue to use the embedded Worker without network I/O.
    pub fn connect_product_http(
        endpoint: &str,
        access_token: String,
        computer_id: String,
        trusted_ca_pem: Option<&[u8]>,
    ) -> TalosResult<Self> {
        let origin = reqwest::Url::parse(endpoint).map_err(http_error)?;
        let loopback = origin.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if (origin.scheme() != "https" && !(origin.scheme() == "http" && loopback))
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
            || !origin.username().is_empty()
            || origin.password().is_some()
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Talos Server requires a secure origin.",
            ));
        }
        let mut client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
        if let Some(pem) = trusted_ca_pem {
            for certificate in reqwest::Certificate::from_pem_bundle(pem).map_err(http_error)? {
                client = client.add_root_certificate(certificate);
            }
        }
        let (_events_tx, events) = tokio::sync::mpsc::channel(1);
        let (_notifications_tx, notifications) = tokio::sync::mpsc::channel(1);
        Ok(Self {
            product_http: Some(super::ProductHttpTransport {
                client: client.build().map_err(http_error)?,
                origin,
                scope: super::ProductHttpScope::Computer(computer_id),
                access_token,
                tasks: Default::default(),
            }),
            computer_id: None,
            ownership: super::AppServerOwnership::ExternalTransport,
            child: None,
            stdin: None,
            next_id: 1,
            pending_responses: Default::default(),
            subscription_senders: Default::default(),
            pending_subscription_events: Default::default(),
            events,
            notifications,
            actor_credentials: None,
            use_cloud_session_cache: false,
        })
    }
}

impl super::ProductHttpTransport {
    pub(super) fn authorized(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let request = request.bearer_auth(&self.access_token);
        match &self.scope {
            super::ProductHttpScope::Computer(computer) => {
                request.header("x-talos-computer", computer)
            }
            super::ProductHttpScope::SourcePeer(peer) => {
                request.header("x-talos-source-peer", peer)
            }
        }
    }

    /// Only the central account endpoint may establish a fresh Server's cached identity.
    /// Retry the same replayable request once; product/source scope never enters bootstrap.
    pub(super) async fn send_authorized(
        &self,
        request: reqwest::RequestBuilder,
    ) -> TalosResult<reqwest::Response> {
        let retry = request
            .try_clone()
            .ok_or_else(|| TalosError::internal("Product request body is not replayable."))?;
        let response = request.send().await.map_err(http_error)?;
        if response.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Ok(response);
        }
        drop(response);
        let bootstrap = self
            .client
            .get(self.origin.join("v1/user").map_err(http_error)?)
            .bearer_auth(&self.access_token)
            .timeout(APP_SERVER_REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(http_error)?;
        let status = bootstrap.status();
        let data =
            super::response::decode_api_response(bootstrap.json().await.map_err(http_error)?)?;
        if !status.is_success() {
            return Err(TalosError::new(
                ErrorCode::Unauthenticated,
                "Talos Server account bootstrap was rejected.",
            ));
        }
        let account: talos_types::UserCurrentResponse =
            serde_json::from_value(data).map_err(http_error)?;
        if account.user.user_id.trim().is_empty() || account.user.disabled {
            return Err(TalosError::new(
                ErrorCode::Unauthenticated,
                "Talos Server requires an active signed-in account.",
            ));
        }
        retry.send().await.map_err(http_error)
    }

    async fn request<P: serde::Serialize>(&self, method: &str, params: P) -> TalosResult<Value> {
        let params = serde_json::to_value(params).map_err(http_error)?;
        let mut request = talos_app_protocol::requests::ClientRequest::decode(method, Some(params))
            .map_err(super::response::map_app_server_error)?;
        if let super::ProductHttpScope::Computer(target) = &self.scope {
            request.params.bind_computer_target(target)?;
        }
        let (path, body) = match &request.params {
            talos_app_protocol::requests::ClientRequestParams::DesktopOpen(params) => (
                "v1/desktop/open",
                serde_json::to_value(params).map_err(http_error)?,
            ),
            params => (
                "v1/app/request",
                serde_json::to_value(params).map_err(http_error)?,
            ),
        };
        let url = self.origin.join(path).map_err(http_error)?;
        let mut outgoing = self.authorized(self.client.post(url));
        if request.params.is_file_transfer() {
            outgoing = outgoing.header("x-talos-file-transfer", "true");
        }
        let response = self
            .send_authorized(
                outgoing
                    .json(&body)
                    .timeout(APP_SERVER_PLUGIN_REQUEST_TIMEOUT),
            )
            .await?;
        let status = response.status();
        let body = response.json::<Value>().await.map_err(http_error)?;
        let result = super::response::decode_api_response(body);
        if status.is_success() || result.is_err() {
            result
        } else {
            Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Talos Server rejected the product request.",
            ))
        }
    }
}

pub(super) fn http_error(error: impl std::error::Error + Send + Sync + 'static) -> TalosError {
    TalosError::from_source(
        ErrorCode::ServiceUnavailable,
        "Talos Server product API is unavailable.",
        error,
    )
}
