//! SDK request serialization and response correlation; transport policy stays private.

use super::credentials::actor_credentials_for_method;
use super::{AppServerClient, APP_SERVER_REQUEST_TIMEOUT};
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
        let id = self.next_request_id();
        let params = serde_json::to_value(params).map_err(|error| {
            TalosError::internal_with_source(
                "App-server communication failed; technical details were withheld.",
                error,
            )
        })?;
        let mut request = JsonRpcRequest::new(RequestId::Number(id), method, Some(params));
        request.actor_credentials = self.actor_credentials_for_request(method);
        let (tx, rx) = oneshot::channel();
        self.pending_responses.lock().await.insert(id, tx);
        if let Err(error) = self.write_json(&request).await {
            self.pending_responses.lock().await.remove(&id);
            return Err(error);
        }
        let response = match tokio::time::timeout(APP_SERVER_REQUEST_TIMEOUT, rx).await {
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
