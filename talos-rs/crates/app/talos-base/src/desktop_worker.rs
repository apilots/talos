//! Personal Worker identity provisioning and registration for management hosts.
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
    pub sync_host: &'a crate::application_sync::ApplicationSyncHost,
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
            sync_host: &self.sync_host,
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
        let previous_runner = self.store.load_agent_node()?.runner;
        if recover_node_id.is_some() {
            if let Some(worker) = active.take() {
                worker.shutdown().await?;
            }
        }
        let intentional = recover_node_id.is_none() && signed_in.is_none();
        if intentional && self.store.removed_registration_owner()?.is_some() {
            if let Some(worker) = active.take() {
                worker.shutdown().await?;
            }
        }
        let mut result = self
            .prepare_worker_identity(recover_node_id, signed_in)
            .await;
        if intentional
            && result
                .as_ref()
                .is_err_and(|error| error.is_code(ErrorCode::ResourceExpired))
            && self.store.removed_registration_owner()?.is_some()
        {
            if let Some(worker) = active.take() {
                worker.shutdown().await?;
            }
            result = self.prepare_worker_identity(None, None).await;
        }
        let config = match result {
            Ok(config) => config,
            Err(error) => {
                // A rejected competing central claim cannot stop its current owner's Worker.
                if self.config.agent_node.control_plane.is_none() {
                    if let Some(worker) = active.take() {
                        if let Err(cleanup) = worker.shutdown().await {
                            talos_logging::report_error(&cleanup);
                        }
                    }
                }
                return Err(error);
            }
        };
        if previous_runner
            .as_ref()
            .map(AgentNodeRunnerConfig::transport)
            != config
                .agent_node
                .runner
                .as_ref()
                .map(AgentNodeRunnerConfig::transport)
        {
            if let Some(worker) = active.take() {
                worker.shutdown().await?;
            }
        }
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
        if let Some(owner) = self.store.removed_registration_owner()? {
            if signed_in.is_some() || recover_node_id.is_some() {
                return Err(TalosError::localized(
                    talos_error::ErrorLocalization::ComputerRegistrationRemoved,
                ));
            }
            if owner != actor.user_id {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Only the previous owner may reconnect this Computer.",
                ));
            }
            self.sync_host
                .retire_removed_local_registration(&actor)
                .await?;
            self.store.retire_removed_registration(&actor.user_id)?;
        }
        let mut config = self.config.reload()?;
        if config.source_path() != config.talos_paths().config_file {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Desktop enrollment requires the configuration in TALOS_HOME/conf/talos.yaml",
            ));
        }
        let endpoint = config.cloud.agent_base_url.clone();
        let mut existing = self.store.load_agent_node()?.runner;
        let central = config.agent_node.control_plane.is_some();
        let direct_endpoint = central.then(|| endpoint.clone());
        let mut transport_changed = false;
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
            .or(existing.as_ref().map(|runner| &runner.node_id))
            .cloned();
        let (node, credential) = if let Some(node_id) = node_id.as_ref() {
            let mut node = match client.get_node(token, node_id).await {
                Ok(node) => node,
                Err(error) if error.is_code(ErrorCode::ResourceExpired) => {
                    self.sync_host
                        .observe_registration_removal(&actor, node_id)
                        .await?;
                    return Err(error);
                }
                Err(error) => return Err(error),
            };
            if node.status == agent_protocol::NodeStatus::Disabled {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Enable this computer before registering its Worker",
                ));
            }
            if node.mode != NodeMode::Remote
                || node.owner_user_id.as_deref() != Some(&actor.user_id)
                || (!central
                    && node.endpoint.is_some()
                        != existing
                            .as_ref()
                            .is_some_and(|runner| runner.transport() == AgentNodeTransport::Direct))
            {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "Saved computer ownership or transport does not match this Worker",
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
            // A saved UUID always retries registration with its existing credential.
            // Successful lease observation clears any interrupted-save marker;
            // rejected/missing credentials still require recovery of this UUID.
            if central {
                if node.endpoint != direct_endpoint {
                    node = client
                        .update_node(
                            token,
                            talos_types::UpdateNodeRequest {
                                node_id: node.node_id.clone(),
                                display_name: node.display_name.clone(),
                                endpoint: direct_endpoint.clone(),
                                status: node.status,
                            },
                        )
                        .await?;
                }
                if let Some(saved) = existing.as_mut() {
                    transport_changed = saved.transport() != AgentNodeTransport::Direct;
                    saved.transport = Some(AgentNodeTransport::Direct);
                }
            }
            let credential = if recover_node_id.is_some() {
                if !self.store.node_enrollment_pending()? {
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
                        endpoint: direct_endpoint.clone(),
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
                || created.node.endpoint != direct_endpoint
            {
                return Err(TalosError::internal(
                    "Control plane returned an invalid Worker ownership or transport",
                ));
            }
            (created.node, Some(created.credential))
        };
        let runner = existing.unwrap_or_else(|| AgentNodeRunnerConfig {
            transport: Some(if central {
                AgentNodeTransport::Direct
            } else {
                AgentNodeTransport::Outbound
            }),
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
        } else if transport_changed {
            // Preserve the Server-issued UUID and protected credential when retiring
            // an old self-outbound registration in favor of the central direct ingress.
            let token = std::fs::read_to_string(&runner.credential_file).map_err(|error| {
                TalosError::internal_with_source(
                    "Read existing Worker credential; technical details were withheld.",
                    error,
                )
            })?;
            self.store.save_node_identity(&runner, token.trim())?;
        }
        config.agent_node.runner = Some(runner);
        Ok(config)
    }
}
