use super::PluginManagerService;
use agent_protocol::{
    WorkerEnvironmentContext, WorkerPluginChange, WorkerPluginInstall, WorkerPluginPrepared,
    WorkerPluginUpdate,
};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_plugin::PreparedPluginChange;
use talos_types::{PluginInstallResponse, PluginUpdateResponse};

pub(super) struct PendingPluginChange {
    actor: WorkerEnvironmentContext,
    fingerprint: [u8; 32],
    created: Instant,
    prepared: PreparedServiceChange,
}

pub(super) struct PreparedServiceChange {
    pub(super) plugin: PreparedPluginChange,
    pub(super) environment: Option<talos_browser::PreparedBrowserEnvironment>,
    _capacity: tokio::sync::OwnedSemaphorePermit,
}

impl PluginManagerService {
    /// Authenticated private Worker preparation. No execution/configuration reservation is held.
    pub async fn prepare_plugin_change(
        &self,
        actor: WorkerEnvironmentContext,
        change: WorkerPluginChange,
    ) -> TalosResult<WorkerPluginPrepared> {
        let fingerprint = fingerprint(&change)?;
        let prepared = self.prepare_change(change).await?;
        let id = uuid::Uuid::new_v4().to_string();
        let mut pending = self
            .preparations
            .lock()
            .map_err(|_| TalosError::internal("Plugin preparation lock is poisoned."))?;
        pending.retain(|_, change| change.created.elapsed() < Duration::from_secs(300));
        pending.insert(
            id.clone(),
            PendingPluginChange {
                actor,
                fingerprint,
                created: Instant::now(),
                prepared,
            },
        );
        Ok(WorkerPluginPrepared { preparation_id: id })
    }

    pub(super) async fn prepare_change(
        &self,
        change: WorkerPluginChange,
    ) -> TalosResult<PreparedServiceChange> {
        self.preparations
            .lock()
            .map_err(|_| TalosError::internal("Plugin preparation lock is poisoned."))?
            .retain(|_, change| change.created.elapsed() < Duration::from_secs(300));
        let capacity = self
            .preparation_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                TalosError::new(
                    ErrorCode::Conflict,
                    "Too many plugin preparations are pending.",
                )
            })?;
        let manager = self.manager.clone();
        // Keep capacity inside the blocking task until any cancelled download really finishes.
        let (plugin, capacity) = tokio::task::spawn_blocking(move || {
            let plugin = match change {
                WorkerPluginChange::Install(mut request) => {
                    if let talos_types::PluginInstallTarget::ManualMcpServer {
                        server,
                        credentials,
                    } = &mut request.target
                    {
                        super::credential_input::extract_inline(server, credentials)?;
                    }
                    manager.prepare_install(&request)
                }
                WorkerPluginChange::Update(request) => manager.prepare_update(&request),
            }?;
            Ok::<_, TalosError>((plugin, capacity))
        })
        .await
        .map_err(|error| {
            TalosError::internal_with_source("Plugin preparation could not finish.", error)
        })??;
        let environment = match (&self.browser, plugin.browser_requirements()?) {
            (Some(browser), Some(lock)) => browser.prepare_browser_environment(&lock).await?,
            _ => None,
        };
        Ok(PreparedServiceChange {
            plugin,
            environment,
            _capacity: capacity,
        })
    }

    pub(super) async fn activate_prepared_browser(
        &self,
        environment: Option<talos_browser::PreparedBrowserEnvironment>,
    ) -> TalosResult<()> {
        if let (Some(browser), Some(prepared)) = (&self.browser, environment) {
            browser.activate_browser_environment(prepared).await?;
        }
        Ok(())
    }

    fn take_preparation(
        &self,
        actor: &WorkerEnvironmentContext,
        id: &str,
        change: &WorkerPluginChange,
    ) -> TalosResult<PreparedServiceChange> {
        let fingerprint = fingerprint(change)?;
        let mut pending = self
            .preparations
            .lock()
            .map_err(|_| TalosError::internal("Plugin preparation lock is poisoned."))?;
        let prepared = pending.get(id).ok_or_else(expired)?;
        if prepared.actor != *actor || prepared.fingerprint != fingerprint {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "The plugin preparation belongs to another actor or operation.",
            ));
        }
        let prepared = pending.remove(id).ok_or_else(expired)?;
        if prepared.created.elapsed() >= Duration::from_secs(300) {
            return Err(expired());
        }
        Ok(prepared.prepared)
    }

    pub async fn commit_plugin_install(
        &self,
        actor: WorkerEnvironmentContext,
        params: WorkerPluginInstall,
    ) -> TalosResult<PluginInstallResponse> {
        let prepared = self.take_preparation(
            &actor,
            &params.preparation_id,
            &WorkerPluginChange::Install(params.request.clone()),
        )?;
        self.install_prepared(params.request, prepared).await
    }

    pub async fn commit_plugin_update(
        &self,
        actor: WorkerEnvironmentContext,
        params: WorkerPluginUpdate,
    ) -> TalosResult<PluginUpdateResponse> {
        let prepared = self.take_preparation(
            &actor,
            &params.preparation_id,
            &WorkerPluginChange::Update(params.request.clone()),
        )?;
        self.update_prepared(params.request, prepared).await
    }
}

fn fingerprint(change: &WorkerPluginChange) -> TalosResult<[u8; 32]> {
    let bytes = serde_json::to_vec(change).map_err(|error| {
        TalosError::internal_with_source(
            "The plugin change could not be bound to its preparation.",
            error,
        )
    })?;
    Ok(Sha256::digest(bytes).into())
}
fn expired() -> TalosError {
    TalosError::new(
        ErrorCode::Conflict,
        "The plugin preparation expired or its Worker restarted. Prepare it again.",
    )
}

#[cfg(test)]
#[path = "preparation_tests.rs"]
mod tests;
