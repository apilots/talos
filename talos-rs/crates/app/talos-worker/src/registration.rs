use crate::standalone::read_credential;
use crate::{TalosWorker, TalosWorkerResources, WorkerRpcHost};
use agent_client::{
    AgentHarnessRpcAdapter, AgentNodeClient, AgentNodeRunner, AgentSessionDataClient, AgentTlsTrust,
};
use agent_protocol::{NodeAuth, Registration};
use std::sync::Arc;
use talos_config::{AgentNodeRunnerConfig, AgentNodeTransport, Config, YamlConfigStore};
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio_util::sync::CancellationToken;

/// Registration of one Worker. Standalone hosts own execution and transport;
/// management hosts attach registration without transferring that ownership.
pub struct RegisteredWorker {
    owned_worker: Option<TalosWorker>,
    shutdown: CancellationToken,
    runner: Option<tokio::task::JoinHandle<TalosResult<()>>>,
    adapter: AgentHarnessRpcAdapter,
    lease_updates: tokio::sync::watch::Receiver<Option<agent_client::AgentNodeLeaseUpdate>>,
}

impl RegisteredWorker {
    pub async fn start(config: Config) -> TalosResult<Self> {
        Self::start_with_failure_handler(config, None).await
    }

    /// The host handles and reports terminal registration errors when a handler is supplied.
    /// Handled failures stop the runner; successful shutdown remains independent of enrollment.
    pub async fn start_with_failure_handler(
        config: Config,
        on_failure: Option<Arc<dyn Fn(TalosError) + Send + Sync>>,
    ) -> TalosResult<Self> {
        config.agent_node.validate(&config.gateway)?;
        let runner = config.agent_node.runner.clone().ok_or_else(|| {
            TalosError::new(ErrorCode::InvalidRequest, "Worker identity is missing")
        })?;
        let (worker, adapter, node, lease_updates) =
            prepare_registered_worker(config, &runner).await?;
        worker.activate_mcp()?;
        Ok(Self::spawn(
            Some(worker),
            adapter,
            node,
            lease_updates,
            on_failure,
        ))
    }

    /// Verify the protected local credential at its control plane without
    /// exposing it to the caller or starting a registration.
    pub async fn verify_saved_credential(
        config: &Config,
        runner: &AgentNodeRunnerConfig,
        access_token: &str,
        account_id: &str,
    ) -> TalosResult<agent_protocol::NodeInfo> {
        let client = registration_client(config, runner).await?;
        let token = match read_credential(&runner.credential_file, &config.talos_paths().conf_dir) {
            Ok(token) => token,
            Err(error) => {
                // File errors are visible only after the directory establishes ownership.
                let node = client.get_node(access_token, &runner.node_id).await?;
                if node.owner_user_id.as_deref() != Some(account_id) {
                    return Err(TalosError::authorization_denied());
                }
                return Err(error);
            }
        };
        client
            .verify_saved_registration(
                access_token,
                NodeAuth {
                    node_id: runner.node_id.clone(),
                    token,
                },
            )
            .await
    }

    /// Register the host's existing Worker. Detaching preserves local execution,
    /// its private local RPC credential and the MCP runtime.
    pub async fn attach(
        worker: &TalosWorker,
        rpc: Arc<WorkerRpcHost>,
        runner: &AgentNodeRunnerConfig,
        product_owner: talos_types::NodeProductOwner,
        on_failure: Option<Arc<dyn Fn(TalosError) + Send + Sync>>,
    ) -> TalosResult<Self> {
        worker.ensure_execution_host(&rpc)?;
        let config = worker
            .config()
            .read()
            .expect("Worker config lock poisoned")
            .clone();
        let client = registration_client(&config, runner).await?;
        let credential = read_credential(&runner.credential_file, &config.talos_paths().conf_dir)?;
        let adapter = rpc.adapter();
        let resources = AgentSessionDataClient::connect(
            &runner.control_plane_endpoint,
            registration_trust(runner)?,
            NodeAuth {
                node_id: runner.node_id.clone(),
                token: credential.clone(),
            },
        )
        .await?;
        worker.use_remote_resource_rpc(resources.clone());
        let (observer, lease_updates) = lease_observer(
            adapter.clone(),
            Some(resources),
            &config,
            credential.clone(),
        );
        let node = AgentNodeRunner::new(
            client,
            runner.node_id.clone(),
            credential,
            worker.deployments(),
        )
        .with_product_owner(product_owner)
        .with_system_info(worker.system_info().clone())
        .with_lease_observer(observer);
        Ok(Self::spawn(None, adapter, node, lease_updates, on_failure))
    }

    fn spawn(
        owned_worker: Option<TalosWorker>,
        adapter: AgentHarnessRpcAdapter,
        node: AgentNodeRunner,
        lease_updates: tokio::sync::watch::Receiver<Option<agent_client::AgentNodeLeaseUpdate>>,
        on_failure: Option<Arc<dyn Fn(TalosError) + Send + Sync>>,
    ) -> Self {
        let node = node.with_execution_adapter(adapter.clone());
        let shutdown = CancellationToken::new();
        let runner_shutdown = shutdown.clone();
        let task = tokio::spawn(async move {
            let result = node.run(runner_shutdown.clone()).await;
            if let (Some(handler), Err(error)) = (on_failure, &result) {
                handler(error.clone());
                return Ok(());
            }
            result
        });
        Self {
            owned_worker,
            lease_updates,
            shutdown,
            runner: Some(task),
            adapter,
        }
    }

    /// Internal host signal for the existing authenticated registration lease.
    /// It is never logged, serialized, or published as product data.
    pub fn lease_updates(
        &self,
    ) -> tokio::sync::watch::Receiver<Option<agent_client::AgentNodeLeaseUpdate>> {
        self.lease_updates.clone()
    }

    /// Observe a runner failure while an independent host waits for process shutdown.
    pub async fn wait(&mut self) -> TalosResult<()> {
        let result = self
            .runner
            .as_mut()
            .expect("registration task")
            .await
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Worker registration task failed; technical details were withheld.",
                    error,
                )
            })?;
        self.runner.take();
        result
    }

    pub fn is_finished(&self) -> bool {
        self.runner
            .as_ref()
            .is_none_or(tokio::task::JoinHandle::is_finished)
    }

    pub async fn shutdown(mut self) -> TalosResult<()> {
        self.shutdown.cancel();
        self.adapter.clear_registration();
        let execution = match &self.owned_worker {
            Some(worker) => worker.shutdown().await,
            None => self.adapter.drain_disconnected().await,
        };
        // Keep the execution adapter alive until registration and Unregister finish.
        let registration = if let Some(runner) = self.runner.take() {
            runner
                .await
                .map_err(|error| {
                    TalosError::internal_with_source(
                        "Worker registration task failed; technical details were withheld.",
                        error,
                    )
                })
                .and_then(|result| result)
        } else {
            Ok(())
        };
        execution?;
        match registration {
            // A revoked registration has already stopped admission. Report its
            // original failure, but do not make successful host cleanup block
            // owner recovery or sign-out. Other registration failures propagate.
            Err(error)
                if matches!(
                    error.error_code(),
                    ErrorCode::Unauthenticated
                        | ErrorCode::PermissionDenied
                        | ErrorCode::ResourceExpired
                ) =>
            {
                talos_logging::report_error(&error);
                Ok(())
            }
            result => result,
        }
    }
}

impl Drop for RegisteredWorker {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

pub(crate) async fn prepare_registered_worker(
    config: Config,
    runner: &AgentNodeRunnerConfig,
) -> TalosResult<(
    TalosWorker,
    AgentHarnessRpcAdapter,
    AgentNodeRunner,
    tokio::sync::watch::Receiver<Option<agent_client::AgentNodeLeaseUpdate>>,
)> {
    let credential = read_credential(&runner.credential_file, &config.talos_paths().conf_dir)?;
    let node_client = registration_client(&config, runner).await?;
    let trust = registration_trust(runner)?;
    let resources = AgentSessionDataClient::connect(
        &runner.control_plane_endpoint,
        trust,
        NodeAuth {
            node_id: runner.node_id.clone(),
            token: credential.clone(),
        },
    )
    .await?;
    let shared_resources = Arc::new(resources.clone());
    let worker = TalosWorker::build_with_browser_host(
        config.clone(),
        TalosWorkerResources {
            attachment_reader: shared_resources.clone(),
            tool_image_store: shared_resources.clone(),
            agent_control: Some(shared_resources.clone()),
            automation_control: Some(shared_resources),
        },
        talos_types::BrowserHostKind::Native,
    )
    .await?;
    worker.use_remote_resource_rpc(resources.clone());
    worker
        .browser()
        .set_desktop_connector(worker.registered_desktop_connector())?;
    let authorization_owner = worker.plugin_manager();
    let browser_owner = worker.browser();
    let adapter = AgentHarnessRpcAdapter::new(
        runner.node_id.clone(),
        worker.config(),
        worker.harnesses().to_vec(),
    )
    .with_environment(worker.environment())
    .with_resource_binder(worker.resource_binder())
    .with_managed_desktop_admission({
        let browser = worker.browser();
        Arc::new(move |admission, registration| {
            let browser = browser.clone();
            Box::pin(async move {
                use agent_protocol::RegistrationAdmission;
                use talos_browser::ManagedDesktopFence;
                match admission {
                    RegistrationAdmission::StopManaged => {
                        browser
                            .fence_managed_desktop(ManagedDesktopFence::Stop, registration)
                            .await
                    }
                    RegistrationAdmission::SleepManaged { idle_before } => {
                        browser
                            .fence_managed_desktop(
                                ManagedDesktopFence::Sleep { idle_before },
                                registration,
                            )
                            .await
                    }
                    RegistrationAdmission::Unblock => browser.clear_managed_desktop_fence(),
                    RegistrationAdmission::Block => Ok(()),
                }
            })
        })
    })
    .with_registration_revocation_hook(Arc::new(move || {
        authorization_owner.cancel_remote_mcp_authorizations();
        if let Err(error) = browser_owner.registration_revoked() {
            talos_logging::report_error(&error);
        }
    }));
    let (host, _) = worker.execution_host(
        &runner.node_id,
        talos_error::SessionOwnerKind::Server,
        talos_agent::SessionHold::Turn,
        || adapter,
    )?;
    let adapter = host.adapter();
    let (observer, lease_updates) = lease_observer(
        adapter.clone(),
        Some(resources),
        &config,
        credential.clone(),
    );
    let node = AgentNodeRunner::new(
        node_client,
        runner.node_id.clone(),
        credential,
        worker.deployments(),
    )
    .with_product_owner(talos_types::NodeProductOwner::Server)
    .with_system_info(worker.system_info().clone())
    .with_lease_observer(observer);
    Ok((worker, adapter, node, lease_updates))
}

async fn registration_client(
    config: &Config,
    runner: &AgentNodeRunnerConfig,
) -> TalosResult<AgentNodeClient> {
    let mut node_config = config.agent_node.clone();
    node_config.runner = Some(runner.clone());
    node_config.validate(&config.gateway)?;
    match runner.transport() {
        AgentNodeTransport::Outbound => {
            AgentNodeClient::connect_websocket(
                &runner.control_plane_endpoint,
                registration_trust(runner)?,
            )
            .await
        }
        AgentNodeTransport::Direct => {
            AgentNodeClient::connect(&runner.control_plane_endpoint, registration_trust(runner)?)
                .await
        }
    }
}

fn registration_trust(runner: &AgentNodeRunnerConfig) -> TalosResult<AgentTlsTrust> {
    let ca_pem = runner
        .trusted_ca_file
        .as_ref()
        .map(std::fs::read)
        .transpose()
        .map_err(|error| {
            TalosError::internal_with_source(
                "Read Agent node trusted CA; technical details were withheld.",
                error,
            )
        })?;
    Ok(ca_pem.map_or(AgentTlsTrust::SystemRoots, AgentTlsTrust::Certificate))
}

fn lease_observer(
    adapter: AgentHarnessRpcAdapter,
    resources: Option<AgentSessionDataClient>,
    config: &Config,
    credential: String,
) -> (
    agent_client::AgentNodeLeaseObserver,
    tokio::sync::watch::Receiver<Option<agent_client::AgentNodeLeaseUpdate>>,
) {
    // Only the managed home can provision identities or own this recovery state.
    let paths = config.talos_paths();
    let store = (config.source_path() == paths.config_file)
        .then(|| YamlConfigStore::with_env_path(config.source_path(), paths.env_file));
    let (lease_tx, lease_rx) = tokio::sync::watch::channel(None);
    let observer: agent_client::AgentNodeLeaseObserver = Arc::new(move |update| {
        let Some(update) = update else {
            adapter.clear_registration();
            lease_tx.send_replace(None);
            return Ok(());
        };
        let published = update.clone();
        if let Some(resources) = &resources {
            resources.update_auth(update.auth.clone())?;
        }
        adapter.install_node_registration(
            &update.auth.node_id,
            Registration {
                lease_token: update.auth.token,
                validity: update.validity,
                owner_user_id: update.owner_user_id,
            },
            update.deadline,
        )?;
        if let Some(store) = &store {
            store.complete_node_enrollment(&update.auth.node_id, &credential)?;
        }
        lease_tx.send_replace(Some(published));
        Ok(())
    });
    (observer, lease_rx)
}
