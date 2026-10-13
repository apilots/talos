//! Enrollment and lifecycle of the app-server's one embedded cloud Worker.
use crate::desktop_application::DesktopApplication;
use agent_client::{AgentNodeClient, AgentTlsTrust};
use agent_protocol::{CreateNodeRequest, NodeMode, RotateCredentialRequest};
use talos_auth::CloudSessionCache;
use talos_config::{AgentNodeRunnerConfig, AgentNodeTransport};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::DesktopWorkerIdentityResponse;

// Registration credentials have an explicit lifetime; expired credentials require owner recovery.
const CREDENTIAL_LIFETIME_SECONDS: i64 = 365 * 24 * 60 * 60;

/// Borrow the existing host resources; enrollment never creates another Worker.
pub(crate) struct WorkerEnrollment<'a> {
    pub config: &'a talos_config::Config,
    pub store: &'a talos_config::YamlConfigStore,
    pub account: &'a talos_service::AccountService,
    pub auth: &'a std::sync::Arc<dyn talos_auth::AuthService>,
    pub worker: &'a std::sync::Arc<tokio::sync::Mutex<Option<talos_worker::RegisteredWorker>>>,
    pub execution: &'a talos_worker::TalosWorker,
    pub rpc: &'a std::sync::Arc<talos_worker::WorkerRpcHost>,
    pub device_name: &'a str,
    pub worker_failures: &'a tokio::sync::watch::Sender<Option<talos_error::ApiError>>,
}

impl DesktopApplication {
    pub(crate) async fn enroll_worker(
        &self,
        recover_node_id: Option<String>,
    ) -> TalosResult<DesktopWorkerIdentityResponse> {
        WorkerEnrollment {
            config: &self.config,
            store: &self.store,
            account: &self.account,
            auth: &self.auth,
            worker: &self.worker,
            execution: &self.execution,
            rpc: &self.rpc,
            device_name: &self.device_name,
            worker_failures: &self.worker_failures,
        }
        .enroll_worker(recover_node_id, None)
        .await
    }
}

impl WorkerEnrollment<'_> {
    pub(crate) async fn enroll_worker(
        &self,
        recover_node_id: Option<String>,
        signed_in: Option<&talos_auth::RequestIdentity>,
    ) -> TalosResult<DesktopWorkerIdentityResponse> {
        // Serialize host enrollment and shutdown; ConfigStore's barrier also prevents a second process creating a duplicate.
        let mut active = self.worker.lock().await;
        if recover_node_id.is_some() {
            if let Some(worker) = active.take() {
                worker.shutdown().await?;
            }
        }
        let result = self
            .prepare_worker_identity(recover_node_id, signed_in)
            .await;
        let config = match result {
            Ok(config) => config,
            Err(error) => {
                if let Some(worker) = active.take() {
                    if let Err(cleanup) = worker.shutdown().await {
                        talos_logging::report_error(&cleanup);
                    }
                }
                return Err(error);
            }
        };
        if active
            .as_ref()
            .is_some_and(talos_worker::RegisteredWorker::is_finished)
        {
            if let Some(worker) = active.take() {
                worker.shutdown().await?;
            }
        }
        if active.is_none() {
            self.worker_failures.send_replace(None);
            let failures = self.worker_failures.clone();
            *active = Some(
                talos_worker::RegisteredWorker::attach(
                    self.execution,
                    self.rpc.clone(),
                    config
                        .agent_node
                        .runner
                        .as_ref()
                        .expect("prepared identity"),
                    Some(std::sync::Arc::new(move |error| {
                        talos_logging::report_error(&error);
                        failures.send_replace(Some(error.to_api_error()));
                    })),
                )
                .await?,
            );
        }
        let runner = config.agent_node.runner.expect("prepared identity");
        Ok(DesktopWorkerIdentityResponse {
            node_id: Some(runner.node_id),
            control_plane_endpoint: Some(runner.control_plane_endpoint),
        })
    }

    async fn prepare_worker_identity(
        &self,
        recover_node_id: Option<String>,
        signed_in: Option<&talos_auth::RequestIdentity>,
    ) -> TalosResult<talos_config::Config> {
        let cache = CloudSessionCache::for_talos_home(self.config.talos_paths().talos_home);
        let session = if signed_in.is_none() {
            let saved = cache
                .load_candidate(CloudSessionCache::now_ms())?
                .ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::Unauthenticated,
                        "Log in before enrolling this computer",
                    )
                })?;
            if saved.cloud_api_base_url != self.config.cloud.api_base_url {
                return Err(TalosError::new(
                    ErrorCode::Unauthenticated,
                    "Saved login belongs to another authentication service",
                ));
            }
            Some(saved)
        } else {
            None
        };
        let token = signed_in
            .and_then(|actor| actor.access_token.as_deref())
            .or_else(|| {
                session
                    .as_ref()
                    .map(|session| session.access_token.as_str())
            })
            .ok_or_else(|| {
                TalosError::new(
                    ErrorCode::Unauthenticated,
                    "Enrollment requires a signed-in account.",
                )
            })?;
        let actor = match self.auth.verify_access_token(token).await {
            Ok(actor) => actor,
            Err(error) => {
                if matches!(
                    error.error_code(),
                    ErrorCode::Unauthenticated | ErrorCode::ResourceExpired
                ) {
                    self.account
                        .clear_rejected_access_token(token, CloudSessionCache::now_ms())
                        .inspect_err(talos_logging::report_error)?;
                }
                return Err(error);
            }
        };
        if actor.auth_disabled
            || actor.user_id
                != signed_in.map_or_else(
                    || {
                        session
                            .as_ref()
                            .expect("saved session")
                            .user
                            .user_id
                            .as_str()
                    },
                    |actor| actor.user_id.as_str(),
                )
        {
            return Err(TalosError::new(
                ErrorCode::Unauthenticated,
                "Saved login identity does not match the verified account",
            ));
        }
        self.account
            .record_verified_access_token_use(token, CloudSessionCache::now_ms())?;
        let mut config = self.config.reload()?;
        if config.source_path() != config.talos_paths().config_file {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Desktop enrollment requires the configuration in TALOS_HOME/conf/talos.yaml",
            ));
        }
        let endpoint = config.cloud.agent_base_url.clone();
        let existing = self.store.load_agent_node()?.runner;
        if existing
            .as_ref()
            .is_some_and(|runner| runner.control_plane_endpoint != endpoint)
        {
            return Err(TalosError::new(
                ErrorCode::Conflict,
                "Saved Worker belongs to another control plane",
            ));
        }
        let ca_file = existing
            .as_ref()
            .and_then(|runner| runner.trusted_ca_file.as_ref())
            .or(config.cloud.agent_trusted_ca_file.as_ref());
        let trust = ca_file
            .map(std::fs::read)
            .transpose()
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Read Worker control-plane CA; technical details were withheld.",
                    error,
                )
            })?
            .map_or(AgentTlsTrust::SystemRoots, AgentTlsTrust::Certificate);
        let client = AgentNodeClient::connect(&endpoint, trust).await?;
        let expires_at = CloudSessionCache::now_ms() / 1000 + CREDENTIAL_LIFETIME_SECONDS;
        let node_id = recover_node_id
            .as_ref()
            .or(existing.as_ref().map(|runner| &runner.node_id));
        let (node, credential) = if let Some(node_id) = node_id {
            let node = client.get_node(token, node_id).await?;
            if node.status == agent_protocol::NodeStatus::Disabled {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Enable this computer before registering its Worker",
                ));
            }
            if node.mode != NodeMode::Remote
                || node.owner_user_id.as_deref() != Some(&actor.user_id)
                || node.endpoint.is_some()
            {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Saved computer is not an outbound Worker owned by this account",
                ));
            }
            if existing
                .as_ref()
                .is_some_and(|saved| saved.node_id != *node_id)
            {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "Recovery must use the saved computer UUID",
                ));
            }
            if recover_node_id.is_none() && self.store.load_desktop()?.enrollment_pending {
                return Err(TalosError::new(ErrorCode::Conflict, "Worker identity save was not confirmed. Recover the saved computer credential."));
            }
            let credential = if recover_node_id.is_some() {
                if !self.store.load_desktop()?.enrollment_pending {
                    self.store.begin_node_enrollment(Some(node_id))?;
                }
                Some(
                    client
                        .rotate_credential(
                            token,
                            RotateCredentialRequest {
                                node_id: node_id.clone(),
                                expires_at,
                            },
                        )
                        .await?,
                )
            } else {
                if node
                    .credential_expires_at
                    .is_none_or(|expiry| expiry <= CloudSessionCache::now_ms() / 1000)
                {
                    return Err(TalosError::new(
                        ErrorCode::ResourceExpired,
                        "Worker registration credential expired. Recover this computer credential.",
                    ));
                }
                None
            };
            (node, credential)
        } else {
            self.store.begin_node_enrollment(None)?;
            let created = client
                .create_node(
                    token,
                    CreateNodeRequest {
                        display_name: self.device_name.to_owned(),
                        endpoint: None,
                        owner_user_id: None,
                        credential_expires_at: expires_at,
                    },
                )
                .await;
            let created = match created {
                Ok(created) => created,
                Err(error) => {
                    if matches!(
                        error.error_code(),
                        ErrorCode::InvalidRequest
                            | ErrorCode::Unauthenticated
                            | ErrorCode::PermissionDenied
                            | ErrorCode::ResourceExpired
                    ) {
                        self.store
                            .cancel_node_creation()
                            .inspect_err(talos_logging::report_error)?;
                    }
                    return Err(error);
                }
            };
            if created.node.owner_user_id.as_deref() != Some(&actor.user_id)
                || created.node.mode != NodeMode::Remote
                || created.node.endpoint.is_some()
            {
                return Err(TalosError::internal(
                    "Control plane returned an invalid Worker ownership or transport",
                ));
            }
            (created.node, Some(created.credential))
        };
        let runner = existing.unwrap_or_else(|| AgentNodeRunnerConfig {
            transport: Some(AgentNodeTransport::Outbound),
            node_id: node.node_id.clone(),
            control_plane_endpoint: endpoint,
            credential_file: config
                .talos_paths()
                .conf_dir
                .join(format!("{}.credential", node.node_id)),
            trusted_ca_file: config.cloud.agent_trusted_ca_file.clone(),
        });
        if let Some(credential) = credential {
            if credential.node_id != node.node_id {
                return Err(TalosError::internal(
                    "Control plane returned a credential for another computer",
                ));
            }
            self.store.save_node_identity(&runner, &credential.token)?;
        }
        config.agent_node.runner = Some(runner);
        Ok(config)
    }
}
