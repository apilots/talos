use std::path::Path;

use agent_client::AgentSessionDataClient;
use async_trait::async_trait;
use talos_agent_provider_kernel::KernelMemoryProgressStore;
use talos_config::Config;
use talos_error::{ErrorCode, TalosError, TalosResult};

pub async fn run() -> TalosResult<()> {
    talos_upgrade::prepare_before_config_load().inspect_err(talos_logging::report_error)?;
    let config = Config::load().inspect_err(talos_logging::report_error)?;
    let _log_guard = init_logging(&config).inspect_err(talos_logging::report_error)?;
    run_with_config(config)
        .await
        .inspect_err(talos_logging::report_error)
}

async fn run_with_config(config: Config) -> TalosResult<()> {
    if config.agent_node.control_plane.is_some() {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Standalone Workers do not serve a control-plane listener",
        ));
    }
    config.agent_node.validate(&config.gateway)?;
    config.agent_node.runner.as_ref().ok_or_else(|| {
        TalosError::new(
            ErrorCode::InvalidRequest,
            "agent_node.runner is required for talos-worker",
        )
    })?;
    let mut worker = crate::RegisteredWorker::start(config).await?;
    let result =
        tokio::select! { _ = shutdown_signal() => Ok(()), result = worker.wait() => result };
    let cleanup = worker.shutdown().await;
    result.and(cleanup)
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
    talos_logging::init_with_config(config.logging.clone(), &config.timezone)
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
