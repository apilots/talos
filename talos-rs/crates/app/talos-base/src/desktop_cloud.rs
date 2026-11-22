//! Connect native desktop media and forward ordinary remote Worker jobs to their Server.
use std::time::Duration;

use reqwest::{Client, Method, Url};
use serde_json::Value;
use talos_app_protocol::requests::ClientRequestParams;
use talos_config::CloudConfig;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::Actor;
use talos_types::{ApiResponse, UserCurrentResponse};

use super::application_sync::ApplicationSyncHost;

pub(super) const SERVER_TICKET: &str = "server:";

pub(super) struct CloudCall {
    method: Method,
    path: &'static str,
    body: Option<Value>,
}

impl CloudCall {
    pub async fn for_request(
        actor: &Actor,
        request: &ClientRequestParams,
        host: &ApplicationSyncHost,
    ) -> TalosResult<Option<Self>> {
        if actor.auth_disabled {
            return Ok(None);
        }
        let (computer, path, body) = match request {
            ClientRequestParams::BuiltinJobList(job) => {
                (&job.computer_id, "/v1/builtin-job/list", value(job)?)
            }
            ClientRequestParams::BuiltinJobControl(job) => {
                (&job.computer_id, "/v1/builtin-job/control", value(job)?)
            }
            _ => return Ok(None),
        };
        if computer == "embedded" || computer == &host.local_computer_id().await? {
            return Ok(None);
        }
        Ok(Some(Self {
            method: Method::POST,
            path,
            body: Some(body),
        }))
    }
}

pub(super) struct DesktopCloud {
    client: Client,
    origin: Url,
}

impl DesktopCloud {
    pub fn new(config: &CloudConfig) -> TalosResult<Self> {
        let origin = Url::parse(&config.agent_base_url).map_err(|_| invalid_origin())?;
        let host = origin.host_str().ok_or_else(invalid_origin)?;
        let loopback = host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        if (origin.scheme() != "https" && !(origin.scheme() == "http" && loopback))
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
            || !origin.username().is_empty()
            || origin.password().is_some()
        {
            return Err(invalid_origin());
        }
        let mut client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(60));
        if let Some(certificate) =
            talos_service::read_worker_trusted_ca(config.agent_trusted_ca_file.as_deref())?
        {
            for root in
                reqwest::Certificate::from_pem_bundle(&certificate).map_err(request_error)?
            {
                client = client.add_root_certificate(root);
            }
        }
        Ok(Self {
            client: client.build().map_err(request_error)?,
            origin,
        })
    }

    pub async fn send(&self, actor: &Actor, call: CloudCall) -> TalosResult<Value> {
        let token = actor.access_token.as_deref().ok_or_else(|| {
            TalosError::new(
                ErrorCode::Unauthenticated,
                "Sign in to manage a cloud Computer.",
            )
        })?;
        match self.request(token, &call).await {
            Err(error) if error.is_code(ErrorCode::Unauthenticated) => {
                // A fresh Server admits this Desktop's Cloud token through the
                // same explicit bootstrap route used by Web. An auth rejection
                // precedes all effects of the management methods above.
                let identity: UserCurrentResponse = self
                    .request(
                        token,
                        &CloudCall {
                            method: Method::GET,
                            path: "/v1/user",
                            body: None,
                        },
                    )
                    .await?;
                if identity.user.user_id != actor.user_id {
                    return Err(TalosError::authorization_denied());
                }
                self.request(token, &call).await
            }
            result => result,
        }
    }

    async fn request<T: serde::de::DeserializeOwned>(
        &self,
        token: &str,
        call: &CloudCall,
    ) -> TalosResult<T> {
        let url = self.origin.join(call.path).map_err(|_| invalid_origin())?;
        let mut request = self
            .client
            .request(call.method.clone(), url)
            .timeout(Duration::from_secs(60))
            .bearer_auth(token);
        if let Some(body) = &call.body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(request_error)?;
        let status = response.status();
        match response
            .json::<ApiResponse<T>>()
            .await
            .map_err(request_error)?
        {
            ApiResponse::Error { error } => Err(error.into()),
            ApiResponse::Success { data } if status.is_success() => Ok(data),
            ApiResponse::Success { .. } => Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "The Computer Server returned an invalid response.",
            )),
        }
    }
}

fn value(request: &impl serde::Serialize) -> TalosResult<Value> {
    serde_json::to_value(request).map_err(|error| {
        TalosError::internal_with_source("Could not encode the Computer management request.", error)
    })
}

fn invalid_origin() -> TalosError {
    TalosError::new(ErrorCode::InvalidRequest, "Invalid Computer Server origin.")
}

fn request_error(error: reqwest::Error) -> TalosError {
    TalosError::from_source(
        if error.is_timeout() {
            ErrorCode::Timeout
        } else {
            ErrorCode::ServiceUnavailable
        },
        "Could not reach the Computer Server.",
        error.without_url(),
    )
}

#[path = "desktop_cloud_stream.rs"]
mod stream;

#[cfg(test)]
#[path = "desktop_cloud_tests.rs"]
mod tests;
