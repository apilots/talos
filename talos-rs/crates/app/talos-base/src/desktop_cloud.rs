//! Route management requests owned by the configured control plane.
use std::{sync::Mutex, time::Duration};

use reqwest::{Client, Method, Url};
use serde_json::{json, Value};
use talos_app_protocol::requests::ClientRequestParams;
use talos_config::CloudConfig;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::Actor;
use talos_types::{
    ApiResponse, DesktopViewGrant, ManagedWorkerActionRequest, ManagedWorkerResponse,
    UserCurrentResponse,
};

use super::application_sync::ApplicationSyncHost;

pub(super) const SERVER_TICKET: &str = "server:";
const TALO_PATH: &str = "/v1/computers/managed-worker";

pub(super) struct CloudCall {
    method: Method,
    path: String,
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
        let computer = match request {
            ClientRequestParams::BuiltinJobList(request) => Some(request.computer_id.as_str()),
            ClientRequestParams::BuiltinJobControl(request) => Some(request.computer_id.as_str()),
            _ => Self::personal_target(request),
        };
        if let Some(computer) = computer {
            if computer == "embedded" || computer == host.local_computer_id().await? {
                return Ok(None);
            }
        }
        if matches!(request, ClientRequestParams::Browser(_)) && computer.is_none() {
            return Ok(None);
        }
        Self::from_request(request)
    }

    pub fn personal_target(request: &ClientRequestParams) -> Option<&str> {
        match request {
            ClientRequestParams::ModelList(params) => params.node_id.as_deref(),
            ClientRequestParams::ConfigGet(params)
                if params.path == talos_types::ConfigPath::SessionMessageSendPolicy =>
            {
                params.node_id.as_deref()
            }
            ClientRequestParams::ComputerRead(params) => Some(&params.node_id),
            ClientRequestParams::DesktopOpen(params) => Some(&params.node_id),
            ClientRequestParams::Browser(params)
                if !matches!(params.operation, talos_types::BrowserOperation::OpenLocal) =>
            {
                params.node_id.as_deref()
            }
            _ => None,
        }
    }

    fn managed_read() -> Self {
        Self {
            method: Method::GET,
            path: TALO_PATH.to_owned(),
            body: None,
        }
    }

    fn from_request(request: &ClientRequestParams) -> TalosResult<Option<Self>> {
        let call = match request {
            ClientRequestParams::ModelList(p) if p.node_id.is_some() => {
                Self::get_query("/v1/models", p)?
            }
            ClientRequestParams::ConfigGet(p)
                if p.node_id.is_some()
                    && p.path == talos_types::ConfigPath::SessionMessageSendPolicy =>
            {
                Self::get_query(
                    "/v1/config/sessionMessageSendPolicy",
                    &[("nodeId", &p.node_id)],
                )?
            }
            ClientRequestParams::ComputerRead(params) => Self {
                method: Method::GET,
                path: format!("/v1/computers/{}", params.node_id),
                body: None,
            },
            ClientRequestParams::Browser(request) => Self {
                method: Method::POST,
                path: "/v1/browser".into(),
                body: Some(value(request)?),
            },
            ClientRequestParams::DesktopOpen(request) => Self {
                method: Method::POST,
                path: "/v1/desktop/open".into(),
                body: Some(value(request)?),
            },
            ClientRequestParams::BuiltinJobList(request) => Self {
                method: Method::POST,
                path: "/v1/builtin-job/list".to_owned(),
                body: Some(value(request)?),
            },
            ClientRequestParams::BuiltinJobControl(request) => Self {
                method: Method::POST,
                path: "/v1/builtin-job/control".to_owned(),
                body: Some(value(request)?),
            },
            ClientRequestParams::ComputerReadManagedWorker(_) => Self::managed_read(),
            ClientRequestParams::ComputerCreateManagedWorker(_) => Self {
                method: Method::POST,
                path: TALO_PATH.to_owned(),
                body: None,
            },
            ClientRequestParams::ComputerStartManagedWorker(request) => {
                Self::talo_action(request, "start")?
            }
            ClientRequestParams::ComputerStopManagedWorker(request) => {
                Self::talo_action(request, "stop")?
            }
            ClientRequestParams::ComputerResetManagedWorker(request) => {
                Self::talo_action(request, "reset")?
            }
            ClientRequestParams::ComputerDeleteManagedWorker(request) => Self {
                method: Method::POST,
                path: Self::talo_action_path(&request.instance_id, "delete")?,
                body: Some(json!({
                    "expectedGeneration": request.expected_generation,
                    "confirmDataLoss": request.confirm_data_loss,
                })),
            },
            _ => return Ok(None),
        };
        Ok(Some(call))
    }

    fn talo_action(request: &ManagedWorkerActionRequest, action: &str) -> TalosResult<Self> {
        Ok(Self {
            method: Method::POST,
            path: Self::talo_action_path(&request.instance_id, action)?,
            body: Some(json!({ "expectedGeneration": request.expected_generation })),
        })
    }

    fn talo_action_path(instance_id: &str, action: &str) -> TalosResult<String> {
        let id = uuid::Uuid::parse_str(instance_id)
            .map_err(|_| TalosError::new(ErrorCode::InvalidRequest, "Invalid Talo instance ID."))?;
        Ok(format!("{TALO_PATH}/{id}/{action}"))
    }
}

pub(super) struct DesktopCloud {
    client: Client,
    origin: Url,
    // A routing hint from a Server response. Every operation still authenticates with that Server.
    personal_status: Mutex<Option<(String, ManagedWorkerResponse)>>,
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
            personal_status: Mutex::default(),
        })
    }

    fn cached_personal_status<T>(
        &self,
        actor: &Actor,
        read: impl FnOnce(&ManagedWorkerResponse) -> T,
    ) -> Option<T> {
        self.personal_status
            .lock()
            .expect("personal Talo routing lock")
            .as_ref()
            .filter(|(owner, _)| !actor.auth_disabled && owner == &actor.user_id)
            .map(|(_, status)| read(status))
    }

    pub fn knows_personal_node(&self, actor: &Actor, node: &str) -> bool {
        self.cached_personal_status(actor, |status| {
            status
                .instance
                .as_ref()
                .is_some_and(|info| info.node_id == node)
        }) == Some(true)
    }

    pub async fn discover_personal_node(&self, actor: &Actor, node: &str) -> bool {
        self.discover_personal_status(actor)
            .await
            .and_then(|status| status.instance)
            .is_some_and(|info| info.node_id == node)
    }

    pub async fn discover_personal_status(&self, actor: &Actor) -> Option<ManagedWorkerResponse> {
        self.read_personal_status(actor, CloudCall::managed_read())
            .await
            .inspect_err(talos_logging::report_error)
            .ok()
    }

    async fn read_personal_status(
        &self,
        actor: &Actor,
        call: CloudCall,
    ) -> TalosResult<ManagedWorkerResponse> {
        let status: ManagedWorkerResponse = self.send(actor, call).await?;
        *self
            .personal_status
            .lock()
            .expect("personal Talo routing lock") = Some((actor.user_id.clone(), status.clone()));
        Ok(status)
    }

    pub async fn dispatch(
        &self,
        actor: &Actor,
        call: CloudCall,
        request: &ClientRequestParams,
    ) -> TalosResult<Value> {
        if matches!(request, ClientRequestParams::ComputerReadManagedWorker(_)) {
            return value(&self.read_personal_status(actor, call).await?);
        }
        if matches!(request, ClientRequestParams::DesktopOpen(_)) {
            let mut grant: DesktopViewGrant = self.send(actor, call).await?;
            grant.ticket = format!("{SERVER_TICKET}{}", grant.ticket);
            return value(&grant);
        }
        self.send(actor, call).await
    }

    pub async fn send<T: serde::de::DeserializeOwned>(
        &self,
        actor: &Actor,
        call: CloudCall,
    ) -> TalosResult<T> {
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
                            path: "/v1/user".to_owned(),
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
        let url = self.origin.join(&call.path).map_err(|_| invalid_origin())?;
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

#[path = "desktop_cloud_chat.rs"]
mod chat;

#[path = "desktop_cloud_stream.rs"]
mod stream;

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

#[cfg(test)]
#[path = "desktop_cloud_tests.rs"]
mod tests;
