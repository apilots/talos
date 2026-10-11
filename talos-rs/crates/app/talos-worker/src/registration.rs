use crate::standalone::{read_credential, RemoteMemoryProgress};
use crate::{TalosWorker, TalosWorkerResources};
use agent_client::{
    AgentHarnessRpcAdapter, AgentNodeClient, AgentNodeRunner, AgentSessionDataClient, AgentTlsTrust,
};
use agent_protocol::{NodeAuth, Registration};
use std::sync::Arc;
use talos_config::{AgentNodeRunnerConfig, AgentNodeTransport, Config};
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio_util::sync::CancellationToken;

/// A single embedded Worker with its outbound registration lifecycle.
/// Registration and Connect share the same Harness adapter and cloud resource clients.
pub struct RegisteredWorker {
    worker: TalosWorker,
    shutdown: CancellationToken,
    runner: Option<tokio::task::JoinHandle<TalosResult<()>>>,
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
        if runner.transport() != AgentNodeTransport::Outbound {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Desktop cloud workers require outbound transport",
            ));
        }
        let (worker, adapter, node) = prepare_registered_worker(config, &runner).await?;
        let node = node.with_execution_adapter(adapter);
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
        Ok(Self {
            worker,
            shutdown,
            runner: Some(task),
        })
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
        let execution = self.worker.shutdown().await;
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
    let trust = ca_pem.map_or(AgentTlsTrust::SystemRoots, AgentTlsTrust::Certificate);
    let node_client =
        AgentNodeClient::connect(&runner.control_plane_endpoint, trust.clone()).await?;
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
    worker.activate_mcp()?;

    let authorization_owner = worker.plugin_manager();
    let adapter = AgentHarnessRpcAdapter::new(
        runner.node_id.clone(),
        worker.config(),
        worker.harnesses().to_vec(),
    )
    .with_environment(worker.environment())
    .with_registration_revocation_hook(Arc::new(move || {
        authorization_owner.cancel_remote_mcp_authorizations()
    }));
    let observer_adapter = adapter.clone();
    let observer_resources = resources;
    let node = AgentNodeRunner::new(
        node_client,
        runner.node_id.clone(),
        credential,
        worker.deployments(),
    )
    .with_lease_observer(Arc::new(move |update| {
        let Some(update) = update else {
            observer_adapter.clear_registration();
            return Ok(());
        };
        observer_resources.update_auth(update.auth.clone())?;
        observer_adapter.install_registration(
            Registration {
                lease_token: update.auth.token,
                validity: update.validity,
                owner_user_id: update.owner_user_id,
            },
            update.deadline,
        )
    }));
    Ok((worker, adapter, node))
}
