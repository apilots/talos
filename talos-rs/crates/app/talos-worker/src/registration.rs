use crate::standalone::{read_credential, RemoteMemoryProgress};
use crate::{TalosWorker, TalosWorkerResources, WorkerRpcHost};
use agent_client::{
    AgentHarnessRpcAdapter, AgentNodeClient, AgentNodeRunner, AgentSessionDataClient, AgentTlsTrust,
};
use agent_protocol::{NodeAuth, Registration};
use std::sync::Arc;
use talos_config::{AgentNodeRunnerConfig, AgentNodeTransport, Config};
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio_util::sync::CancellationToken;

/// Registration of one Worker. Standalone hosts own execution and transport;
/// management hosts attach registration without transferring that ownership.
pub struct RegisteredWorker {
    owned_worker: Option<TalosWorker>,
    shutdown: CancellationToken,
    runner: Option<tokio::task::JoinHandle<TalosResult<()>>>,
    rpc: Arc<WorkerRpcHost>,
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
        let rpc_config = config.agent_node.worker_rpc.clone();
        let (worker, adapter, node) = prepare_registered_worker(config, &runner).await?;
        let rpc = Arc::new(WorkerRpcHost::bind(&rpc_config, adapter.clone()).await?);
        worker.activate_mcp()?;
        Ok(Self::spawn(
            Some(worker),
            rpc,
            node,
            runner.transport(),
            on_failure,
        ))
    }

    /// Register the host's existing Worker and listener. Detaching never stops
    /// its local RPC credential, local executions, MCP runtime or listener.
    pub async fn attach(
        worker: &TalosWorker,
        rpc: Arc<WorkerRpcHost>,
        runner: &AgentNodeRunnerConfig,
        on_failure: Option<Arc<dyn Fn(TalosError) + Send + Sync>>,
    ) -> TalosResult<Self> {
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
        let node = AgentNodeRunner::new(
            client,
            runner.node_id.clone(),
            credential,
            worker.deployments(),
        )
        .with_system_info(worker.system_info().clone())
        .with_lease_observer(lease_observer(adapter, Some(resources)));
        Ok(Self::spawn(None, rpc, node, runner.transport(), on_failure))
    }

    fn spawn(
        owned_worker: Option<TalosWorker>,
        rpc: Arc<WorkerRpcHost>,
        node: AgentNodeRunner,
        transport: AgentNodeTransport,
        on_failure: Option<Arc<dyn Fn(TalosError) + Send + Sync>>,
    ) -> Self {
        let node = match transport {
            AgentNodeTransport::Outbound => node.with_execution_adapter(rpc.adapter()),
            AgentNodeTransport::Direct => node,
        };
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
            shutdown,
            runner: Some(task),
            rpc,
        }
    }

    /// Observe a runner failure while an independent host waits for process shutdown.
    pub async fn wait(&mut self) -> TalosResult<()> {
        let result = tokio::select! {
            result = self.runner.as_mut().expect("registration task") => result,
            _ = self.rpc.stopped() => return self.rpc.shutdown().await,
        }
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
        self.rpc.is_finished()
            || self
                .runner
                .as_ref()
                .is_none_or(tokio::task::JoinHandle::is_finished)
    }

    pub async fn shutdown(mut self) -> TalosResult<()> {
        self.shutdown.cancel();
        self.rpc.adapter().clear_registration();
        let (execution, transport) = match &self.owned_worker {
            Some(worker) => (worker.shutdown().await, self.rpc.shutdown().await),
            None => (self.rpc.adapter().drain_disconnected().await, Ok(())),
        };
        let registration = if let Some(runner) = self.runner.take() {
            runner.await.map_err(|error| {
                TalosError::internal_with_source(
                    "Worker registration task failed; technical details were withheld.",
                    error,
                )
            })?
        } else {
            Ok(())
        };
        execution?;
        transport?;
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
) -> TalosResult<(TalosWorker, AgentHarnessRpcAdapter, AgentNodeRunner)> {
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
    let worker = TalosWorker::build(
        config.clone(),
        TalosWorkerResources {
            session_history_reader: shared_resources.clone(),
            memory_progress_store: Arc::new(RemoteMemoryProgress(resources.clone())),
            attachment_reader: shared_resources.clone(),
            tool_image_store: shared_resources.clone(),
            agent_control: Some(shared_resources),
        },
    )
    .await?;
    worker.use_remote_resource_rpc(resources.clone());
    let authorization_owner = worker.plugin_manager();
    let adapter = AgentHarnessRpcAdapter::new(
        runner.node_id.clone(),
        worker.config(),
        worker.harnesses().to_vec(),
    )
    .with_environment(worker.environment())
    .with_resource_binder(worker.resource_binder())
    .with_registration_revocation_hook(Arc::new(move || {
        authorization_owner.cancel_remote_mcp_authorizations()
    }));
    let node = AgentNodeRunner::new(
        node_client,
        runner.node_id.clone(),
        credential,
        worker.deployments(),
    )
    .with_system_info(worker.system_info().clone())
    .with_lease_observer(lease_observer(adapter.clone(), Some(resources)));
    Ok((worker, adapter, node))
}

async fn registration_client(
    config: &Config,
    runner: &AgentNodeRunnerConfig,
) -> TalosResult<AgentNodeClient> {
    let mut node_config = config.agent_node.clone();
    node_config.runner = Some(runner.clone());
    node_config.validate(&config.gateway)?;
    AgentNodeClient::connect(&runner.control_plane_endpoint, registration_trust(runner)?).await
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
) -> agent_client::AgentNodeLeaseObserver {
    Arc::new(move |update| {
        let Some(update) = update else {
            adapter.clear_registration();
            return Ok(());
        };
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
        )
    })
}
