use std::sync::Arc;

use talos_config::{domains::managed_worker::WorkerRuntimeKind, Config};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_fs::{FileLock, FileSystem};
use talos_service::{
    connect_docker_worker_runtime, connect_kubernetes_worker_runtime, read_worker_trusted_ca,
    AgentNodeService,
};
use talos_worker_runtime::ManagedWorkerRuntime;

/// Attach the configured Worker Runtime to the authenticated management Server.
pub(crate) async fn configure(
    config: &Config,
    nodes: &AgentNodeService,
) -> TalosResult<Option<(AgentNodeService, FileLock)>> {
    let Some(runtime) = &config.talos.worker.runtime else {
        return Ok(None);
    };
    let lock = talos_fs::std_fs()
        .try_file_lock(&config.talos_paths().data_dir.join("managed-worker.lock"))?
        .ok_or_else(|| {
            TalosError::new(
                ErrorCode::Conflict,
                "Another Server already manages Talo for this home.",
            )
        })?;
    let backend: Arc<dyn ManagedWorkerRuntime> = match runtime.kind {
        WorkerRuntimeKind::DockerEngine => {
            Arc::new(connect_docker_worker_runtime(&runtime.docker_engine).await?)
        }
        WorkerRuntimeKind::Kubernetes => Arc::new(
            connect_kubernetes_worker_runtime(&runtime.kubernetes, &config.talos_paths(), None)
                .await?,
        ),
    };
    let connection = runtime.connection(runtime.kind);
    let trusted_ca = read_worker_trusted_ca(connection.trusted_ca_file.as_deref())?;
    nodes.install_managed_worker_runtime(
        backend,
        runtime.clone(),
        config.talos.server.runtime.clone(),
        trusted_ca,
    )?;
    Ok(Some((nodes.clone(), lock)))
}
