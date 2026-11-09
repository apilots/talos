use std::sync::Arc;

use talos_config::Config;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_fs::{FileLock, FileSystem};
use talos_service::AgentNodeService;
use talos_worker_runtime::{DockerEngine, DEFAULT_SECCOMP_PROFILE};

/// Attach the single-host manager only to the public authenticated Server.
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
    let engine = DockerEngine::connect(&runtime.docker_engine.socket_path, DEFAULT_SECCOMP_PROFILE)
        .map_err(engine_unavailable)?;
    engine.check().await.map_err(engine_unavailable)?;
    let trusted_ca = runtime
        .trusted_ca_file
        .as_ref()
        .map(std::fs::read)
        .transpose()
        .map_err(|error| {
            TalosError::internal_with_source("Read the managed Worker trusted CA.", error)
        })?;
    nodes.install_managed_worker_runtime(
        Arc::new(engine),
        runtime.clone(),
        config.talos.server.runtime.clone(),
        trusted_ca,
    )?;
    Ok(Some((nodes.clone(), lock)))
}

fn engine_unavailable(error: talos_worker_runtime::RuntimeError) -> TalosError {
    TalosError::from_source(
        ErrorCode::ServiceUnavailable,
        "The local Docker Engine is unavailable.",
        error,
    )
}
