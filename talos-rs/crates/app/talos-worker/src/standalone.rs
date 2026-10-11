use std::path::Path;

use agent_client::AgentSessionDataClient;
use agent_pb as pb;
use async_trait::async_trait;
use talos_agent_provider_kernel::KernelMemoryProgressStore;
use talos_config::Config;
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio_util::sync::CancellationToken;
use tonic::transport::{Identity, Server, ServerTlsConfig};

pub async fn run() -> TalosResult<()> {
    let config = Config::load().inspect_err(talos_logging::report_error)?;
    let _log_guard = init_logging(&config).inspect_err(talos_logging::report_error)?;
    run_with_config(config)
        .await
        .inspect_err(talos_logging::report_error)
}

async fn run_with_config(config: Config) -> TalosResult<()> {
    config.agent_node.validate(&config.gateway)?;
    let runner = config.agent_node.runner.clone().ok_or_else(|| {
        TalosError::new(
            ErrorCode::InvalidRequest,
            "agent_node.runner is required for talos-worker",
        )
    })?;
    if runner.transport() == talos_config::AgentNodeTransport::Outbound {
        let mut worker = crate::RegisteredWorker::start(config).await?;
        let result =
            tokio::select! { _ = shutdown_signal() => Ok(()), result = worker.wait() => result };
        let cleanup = worker.shutdown().await;
        return result.and(cleanup);
    }
    let (worker, adapter, node) =
        crate::registration::prepare_registered_worker(config, &runner).await?;
    let address = runner.bind_address.parse().map_err(|error| {
        TalosError::from_source(
            ErrorCode::InvalidRequest,
            "Agent Harness bind address is invalid",
            error,
        )
    })?;
    let certificate = std::fs::read(&runner.tls_certificate_file).map_err(|error| {
        TalosError::internal_with_source(
            "Read Agent Harness TLS certificate; technical details were withheld.",
            error,
        )
    })?;
    let private_key = std::fs::read(&runner.tls_private_key_file).map_err(|error| {
        TalosError::internal_with_source(
            "Read Agent Harness TLS key; technical details were withheld.",
            error,
        )
    })?;
    let shutdown = CancellationToken::new();
    let signal_shutdown = shutdown.clone();
    let signal_task = tokio::spawn(async move {
        shutdown_signal().await;
        signal_shutdown.cancel();
    });
    let server_shutdown = shutdown.clone();
    let mut server = tokio::spawn(async move {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        Server::builder()
            .tls_config(
                ServerTlsConfig::new().identity(Identity::from_pem(certificate, private_key)),
            )
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Configure Agent Harness TLS; technical details were withheld.",
                    error,
                )
            })?
            .add_service(
                pb::worker_environment_rpc_server::WorkerEnvironmentRpcServer::new(adapter.clone()),
            )
            .add_service(pb::agent_harness_rpc_server::AgentHarnessRpcServer::new(
                adapter,
            ))
            .serve_with_shutdown(address, server_shutdown.cancelled_owned())
            .await
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Serve Agent Harness RPC; technical details were withheld.",
                    error,
                )
            })
    });
    let node_shutdown = shutdown.clone();
    let mut node = tokio::spawn(node.run(node_shutdown));
    let (result, server_finished) = tokio::select! {
        result = &mut server => (result.map_err(|error| TalosError::internal_with_source(
            "Agent Harness task failed; technical details were withheld.", error))?, true),
        result = &mut node => (result.map_err(|error| TalosError::internal_with_source(
            "Agent node runner task failed; technical details were withheld.", error))?, false),
    };
    shutdown.cancel();
    // Local execution must not wait for a peer to drain its HTTP/2 connection.
    let execution_result = worker.shutdown().await;
    let peer_result = if server_finished {
        node.await.map_err(|error| {
            TalosError::internal_with_source(
                "Agent node runner task failed; technical details were withheld.",
                error,
            )
        })?
    } else {
        drain_harness_server(server).await
    };
    signal_task.abort();
    result.and(peer_result).and(execution_result)
}

async fn drain_harness_server(
    mut server: tokio::task::JoinHandle<TalosResult<()>>,
) -> TalosResult<()> {
    match tokio::time::timeout(std::time::Duration::from_secs(5), &mut server).await {
        Ok(joined) => joined.map_err(|error| {
            TalosError::internal_with_source(
                "Agent Harness task failed; technical details were withheld.",
                error,
            )
        })?,
        Err(error) => {
            talos_logging::report_error(&TalosError::from_source(
                ErrorCode::Timeout,
                "Agent Harness transport drain timed out",
                error,
            ));
            server.abort();
            match server.await {
                Err(error) if error.is_cancelled() => Ok(()),
                Err(error) => Err(TalosError::internal_with_source(
                    "Agent Harness task failed; technical details were withheld.",
                    error,
                )),
                Ok(result) => result,
            }
        }
    }
}

pub(crate) struct RemoteMemoryProgress(pub(crate) AgentSessionDataClient);

#[async_trait]
impl KernelMemoryProgressStore for RemoteMemoryProgress {
    async fn processed_until_event_seq(&self, session_id: &str) -> TalosResult<u64> {
        self.0.processed_until_event_seq(session_id).await
    }

    async fn advance_processed_until_event_seq(
        &self,
        session_id: &str,
        expected_until_seq: u64,
        next_until_seq: u64,
    ) -> TalosResult<()> {
        self.0
            .advance_processed_until_event_seq(session_id, expected_until_seq, next_until_seq)
            .await
    }
}

fn init_logging(config: &Config) -> TalosResult<talos_logging::LogGuard> {
    let mut logging = config.logging.clone();
    if logging.timezone.is_none() {
        logging.timezone = Some(config.timezone.clone());
    }
    talos_logging::init_with_config(logging)
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut terminate = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

pub(crate) fn read_credential(path: &Path, conf_dir: &Path) -> TalosResult<String> {
    if !path.is_absolute() {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Agent node credential file must be an absolute path",
        ));
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        TalosError::internal_with_source(
            "Read Agent node credential metadata; technical details were withheld.",
            error,
        )
    })?;
    if !metadata.file_type().is_file() {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Agent node credential must be a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o777 != 0o600 {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Agent node credential file must have mode 0600",
            ));
        }
    }
    let actual = path.canonicalize().map_err(|error| {
        TalosError::internal_with_source(
            "Resolve Agent node credential file; technical details were withheld.",
            error,
        )
    })?;
    let config_root = conf_dir.canonicalize().map_err(|error| {
        TalosError::internal_with_source(
            "Resolve Agent node configuration directory; technical details were withheld.",
            error,
        )
    })?;
    if !actual.starts_with(config_root) {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Agent node credential file must be inside the configuration directory",
        ));
    }
    let token = std::fs::read_to_string(path).map_err(|error| {
        TalosError::internal_with_source(
            "Read Agent node credential; technical details were withheld.",
            error,
        )
    })?;
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Agent node credential file is empty",
        ));
    }
    Ok(token)
}

#[cfg(test)]
#[path = "standalone_tests.rs"]
mod tests;
