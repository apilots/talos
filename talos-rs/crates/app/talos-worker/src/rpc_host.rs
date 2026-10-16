//! Transport lifecycle for an existing Worker; it never builds a second runtime.

use agent_client::{AgentEnvironmentClient, AgentHarnessRpcAdapter, RemoteAgentClient};
use agent_pb as pb;
use agent_protocol::NodeAuth;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use talos_config::WorkerRpcConfig;
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::{net::TcpListener, sync::Mutex, task::JoinHandle};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;
use tonic::transport::{Identity, Server, ServerTlsConfig};

/// Bound RPC listener shared by embedded, direct and outbound hosts.
/// Owners stop execution before draining transport; dropping also stops admission.
pub struct WorkerRpcHost {
    address: SocketAddr,
    adapter: AgentHarnessRpcAdapter,
    shutdown: CancellationToken,
    finished: CancellationToken,
    server: Mutex<Option<JoinHandle<TalosResult<()>>>>,
}

impl WorkerRpcHost {
    /// Binds before returning, so registration cannot publish an unbound listener.
    pub async fn bind(
        config: &WorkerRpcConfig,
        adapter: AgentHarnessRpcAdapter,
    ) -> TalosResult<Self> {
        let address = config.address()?;
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let mut builder = Server::builder();
        if let Some(tls) = &config.tls {
            let certificate = tokio::fs::read(&tls.certificate_file)
                .await
                .map_err(|error| {
                    TalosError::internal_with_source(
                        "Read Worker TLS certificate; technical details were withheld.",
                        error,
                    )
                })?;
            let key = tokio::fs::read(&tls.private_key_file)
                .await
                .map_err(|error| {
                    TalosError::internal_with_source(
                        "Read Worker TLS key; technical details were withheld.",
                        error,
                    )
                })?;
            builder = builder
                .tls_config(ServerTlsConfig::new().identity(Identity::from_pem(certificate, key)))
                .map_err(|error| {
                    TalosError::internal_with_source(
                        "Configure Worker TLS; technical details were withheld.",
                        error,
                    )
                })?;
        }
        let listener = TcpListener::bind(address).await.map_err(|error| {
            TalosError::from_source(
                ErrorCode::ServiceUnavailable,
                "Worker RPC listener is unavailable",
                error,
            )
        })?;
        let address = listener.local_addr().map_err(|error| {
            TalosError::internal_with_source(
                "Read Worker listener address; technical details were withheld.",
                error,
            )
        })?;
        let shutdown = CancellationToken::new();
        let stop = shutdown.clone();
        let finished = CancellationToken::new();
        let task_finished = finished.clone();
        let server_adapter = adapter.clone();
        let server = tokio::spawn(async move {
            let _finished = task_finished.drop_guard();
            builder
                .add_service(
                    pb::worker_environment_rpc_server::WorkerEnvironmentRpcServer::new(
                        server_adapter.clone(),
                    ),
                )
                .add_service(pb::agent_harness_rpc_server::AgentHarnessRpcServer::new(
                    server_adapter,
                ))
                .serve_with_incoming_shutdown(
                    TcpListenerStream::new(listener),
                    stop.cancelled_owned(),
                )
                .await
                .map_err(|error| {
                    TalosError::internal_with_source(
                        "Serve Worker RPC; technical details were withheld.",
                        error,
                    )
                })
        });
        tracing::info!(%address, "Worker RPC listener ready");
        Ok(Self {
            address,
            adapter,
            shutdown,
            finished,
            server: Mutex::new(Some(server)),
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Reuses this authenticated Worker on a host's existing TLS ingress.
    pub fn adapter(&self) -> AgentHarnessRpcAdapter {
        self.adapter.clone()
    }

    /// Wait for transport failure without consuming its eventual shutdown result.
    pub async fn stopped(&self) {
        self.finished.cancelled().await;
    }

    pub fn is_finished(&self) -> bool {
        self.finished.is_cancelled()
    }

    pub async fn shutdown(&self) -> TalosResult<()> {
        self.adapter.clear_local_auth();
        self.adapter.clear_registration();
        self.shutdown.cancel();
        let Some(mut task) = self.server.lock().await.take() else {
            return Ok(());
        };
        match tokio::time::timeout(Duration::from_secs(5), &mut task).await {
            Ok(result) => result.map_err(|error| {
                TalosError::internal_with_source(
                    "Worker RPC task failed; technical details were withheld.",
                    error,
                )
            })?,
            Err(error) => {
                task.abort();
                let joined = task.await;
                if let Err(join_error) = joined {
                    if !join_error.is_cancelled() {
                        return Err(TalosError::internal_with_source(
                            "Worker RPC task failed; technical details were withheld.",
                            join_error,
                        ));
                    }
                }
                Err(TalosError::from_source(
                    ErrorCode::Timeout,
                    "Worker RPC transport drain timed out",
                    error,
                ))
            }
        }
    }
}

impl Drop for WorkerRpcHost {
    fn drop(&mut self) {
        self.adapter.clear_local_auth();
        self.adapter.clear_registration();
        self.shutdown.cancel();
    }
}

impl crate::TalosWorker {
    /// Builds one protected loopback entry to these exact Harness instances.
    /// The private process credential is not a cloud lease or a public grant.
    pub async fn start_local_rpc(&self) -> TalosResult<(Arc<WorkerRpcHost>, NodeAuth)> {
        let auth = NodeAuth {
            node_id: "embedded".into(),
            token: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
        };
        let adapter = AgentHarnessRpcAdapter::new(
            auth.node_id.clone(),
            self.config(),
            self.harnesses().to_vec(),
        )
        .with_environment(self.environment())
        .with_resource_binder(self.resource_binder())
        .with_registration_revocation_hook({
            let plugins = self.plugin_manager();
            Arc::new(move || plugins.cancel_remote_mcp_authorizations())
        });
        adapter.install_local_auth(&auth)?;
        let config = self
            .config()
            .read()
            .expect("Worker config lock poisoned")
            .agent_node
            .worker_rpc
            .clone();
        if config.tls.is_some() || !config.address()?.ip().is_loopback() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "Embedded Worker RPC requires a loopback listener",
            ));
        }
        Ok((Arc::new(WorkerRpcHost::bind(&config, adapter).await?), auth))
    }

    pub async fn local_rpc_harnesses(
        &self,
        host: &WorkerRpcHost,
        auth: &NodeAuth,
        coordinator_id: &str,
    ) -> TalosResult<Vec<(String, Arc<dyn agent_protocol::AgentHarness>)>> {
        let mut harnesses = Vec::new();
        for deployment in self.deployments() {
            let client = RemoteAgentClient::connect_loopback(
                host.address(),
                deployment.agent_id.clone(),
                auth.clone(),
            )
            .await?
            .with_coordinator_id(coordinator_id.to_owned())?;
            harnesses.push((
                deployment.agent_id,
                Arc::new(client) as Arc<dyn agent_protocol::AgentHarness>,
            ));
        }
        Ok(harnesses)
    }

    pub async fn local_rpc_environment(
        &self,
        host: &WorkerRpcHost,
        auth: NodeAuth,
    ) -> TalosResult<Arc<dyn agent_protocol::WorkerEnvironment>> {
        Ok(Arc::new(
            AgentEnvironmentClient::connect_loopback(host.address(), auth).await?,
        ))
    }
}
