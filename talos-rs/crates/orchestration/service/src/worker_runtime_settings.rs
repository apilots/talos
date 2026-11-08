use std::time::Duration;

use talos_auth::UserRole;
use talos_config::domains::managed_worker::{
    read_pasted_kubeconfig, write_pasted_kubeconfig, DockerEngineConfig, KubeconfigSource,
    KubernetesConfig, KubernetesCredential, WorkerRuntimeKind,
};
use talos_config::{Config, TalosPaths, WorkerRuntimeConfig, YamlConfigStore};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    WorkerKubeconfigAuthentication, WorkerKubeconfigSource, WorkerKubernetesSettings,
    WorkerRuntimeBackend, WorkerRuntimeCandidate, WorkerRuntimeSettingsResponse,
};
use talos_worker_runtime::{
    DockerEngine, Kubernetes, KubernetesOptions, RuntimeError, DEFAULT_SECCOMP_PROFILE,
};
use uuid::Uuid;

use crate::{Actor, TalosService};

pub(crate) const KUBERNETES_HOME_BYTES: u64 = 100_000_000_000;

const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_KUBECONFIG_BYTES: usize = 256 * 1024;

impl TalosService {
    /// Read Server-owned settings independently of the selected Computer.
    pub fn read_worker_runtime_settings(
        &self,
        actor: &Actor,
        active: &Config,
    ) -> TalosResult<WorkerRuntimeSettingsResponse> {
        require_admin(actor)?;
        let saved = if active.talos.worker.runtime.is_some() {
            active.reload()?.talos.worker.runtime
        } else {
            None
        };
        Ok(settings(active, saved.as_ref()))
    }

    /// Check a candidate's configuration, connectivity and permissions without saving it.
    pub async fn validate_worker_runtime_settings(
        &self,
        actor: &Actor,
        active: &Config,
        request: WorkerRuntimeCandidate,
    ) -> TalosResult<()> {
        require_admin(actor)?;
        let saved = load_saved(active)?;
        let (candidate, inline) = candidate(&saved, request)?;
        preflight(active, &candidate, inline.as_deref()).await
    }

    /// Save a preflighted candidate for the next controlled Server restart.
    pub async fn update_worker_runtime_settings(
        &self,
        actor: &Actor,
        active: &Config,
        request: WorkerRuntimeCandidate,
    ) -> TalosResult<WorkerRuntimeSettingsResponse> {
        require_admin(actor)?;
        self.nodes
            .with_managed_runtime_update(|has_instances| async move {
                let saved = load_saved(active)?;
                let (candidate, inline) = candidate(&saved, request)?;
                let response = settings(active, Some(&candidate));
                if response.restart_required && has_instances {
                    return Err(TalosError::new(
                        ErrorCode::Conflict,
                        "The runtime target cannot change while managed Talos still own resources.",
                    ));
                }
                preflight(active, &candidate, inline.as_deref()).await?;
                if let Some(contents) = inline {
                    let reference = candidate.kubernetes.pasted_kubeconfig.ok_or_else(|| {
                        TalosError::internal("The runtime credential reference is missing.")
                    })?;
                    write_pasted_kubeconfig(&active.talos_paths(), reference, &contents)?;
                }
                let persisted = if saved != candidate {
                    YamlConfigStore::new(active.source_path())
                        .save_worker_runtime_selection(&saved, &candidate)
                } else {
                    Ok(())
                };
                if let Err(error) = persisted {
                    // Replacement can succeed before directory sync fails.
                    let pending = active.reload().map_or(true, |disk| {
                        runtime_changed(
                            active.talos.worker.runtime.as_ref(),
                            disk.talos.worker.runtime.as_ref(),
                        )
                    });
                    return Ok((Err(error), pending));
                }
                let restart_required = response.restart_required;
                Ok((Ok(response), restart_required))
            })
            .await?
    }
}

fn require_admin(actor: &Actor) -> TalosResult<()> {
    if actor.auth_disabled || actor.role != UserRole::Admin {
        return Err(TalosError::new(
            ErrorCode::PermissionDenied,
            "Only an authenticated Server administrator can manage Worker runtime settings.",
        ));
    }
    Ok(())
}

fn load_saved(active: &Config) -> TalosResult<WorkerRuntimeConfig> {
    let unconfigured = || {
        TalosError::new(
            ErrorCode::ServiceUnavailable,
            "A Server operator must configure the Worker image and control plane before selecting a runtime.",
        )
    };
    if active.talos.worker.runtime.is_none() {
        return Err(unconfigured());
    }
    active
        .reload()?
        .talos
        .worker
        .runtime
        .ok_or_else(unconfigured)
}

fn settings(active: &Config, saved: Option<&WorkerRuntimeConfig>) -> WorkerRuntimeSettingsResponse {
    let active = active.talos.worker.runtime.as_ref();
    let backend = |config: &WorkerRuntimeConfig| match config.kind {
        WorkerRuntimeKind::DockerEngine => WorkerRuntimeBackend::DockerEngine,
        WorkerRuntimeKind::Kubernetes => WorkerRuntimeBackend::Kubernetes,
    };
    WorkerRuntimeSettingsResponse {
        active_backend: active.map(backend),
        configured_backend: saved.map(backend),
        docker_socket_path: saved.map(|runtime| {
            runtime
                .docker_engine
                .socket_path
                .to_string_lossy()
                .into_owned()
        }),
        kubernetes: saved.map(|runtime| {
            let kube = &runtime.kubernetes;
            WorkerKubernetesSettings {
                namespace: kube.namespace.clone(),
                storage_class: kube.storage_class.clone(),
                authentication_source: match kube.credential_source() {
                    KubeconfigSource::InCluster => WorkerKubeconfigSource::InCluster,
                    KubeconfigSource::Mounted => WorkerKubeconfigSource::Mounted,
                    KubeconfigSource::Pasted => WorkerKubeconfigSource::Pasted,
                },
                mounted_kubeconfig_path: kube
                    .kubeconfig_path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned()),
                context: kube.kubeconfig_context.clone(),
            }
        }),
        restart_required: runtime_changed(active, saved),
    }
}

fn runtime_changed(
    active: Option<&WorkerRuntimeConfig>,
    saved: Option<&WorkerRuntimeConfig>,
) -> bool {
    match (active, saved) {
        (None, None) => false,
        (Some(active), Some(saved)) => {
            active.kind != saved.kind
                || active.image != saved.image
                || active.control_plane_endpoint != saved.control_plane_endpoint
                || active.trusted_ca_file != saved.trusted_ca_file
                || match active.kind {
                    WorkerRuntimeKind::DockerEngine => active.docker_engine != saved.docker_engine,
                    WorkerRuntimeKind::Kubernetes => active.kubernetes != saved.kubernetes,
                }
        }
        _ => true,
    }
}

fn candidate(
    saved: &WorkerRuntimeConfig,
    request: WorkerRuntimeCandidate,
) -> TalosResult<(WorkerRuntimeConfig, Option<String>)> {
    let mut candidate = saved.clone();
    let WorkerRuntimeCandidate::Kubernetes {
        namespace,
        storage_class,
        authentication,
    } = request
    else {
        candidate.kind = WorkerRuntimeKind::DockerEngine;
        return Ok((candidate, None));
    };
    candidate.kind = WorkerRuntimeKind::Kubernetes;
    let kubernetes = &mut candidate.kubernetes;
    kubernetes.namespace = namespace.trim().into();
    kubernetes.storage_class = storage_class.trim().into();
    let (source, context, inline) = match authentication {
        WorkerKubeconfigAuthentication::InCluster => (KubeconfigSource::InCluster, None, None),
        WorkerKubeconfigAuthentication::Mounted { context } => {
            (KubeconfigSource::Mounted, context, None)
        }
        WorkerKubeconfigAuthentication::Pasted {
            context,
            kubeconfig,
        } => {
            if kubeconfig
                .as_ref()
                .is_some_and(|text| text.trim().is_empty() || text.len() > MAX_KUBECONFIG_BYTES)
            {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "Paste a nonempty kubeconfig of at most 256 KiB.",
                ));
            }
            (KubeconfigSource::Pasted, context, kubeconfig)
        }
    };
    if kubernetes.credential_source() != source {
        kubernetes.kubeconfig_source = Some(source);
    }
    kubernetes.kubeconfig_context = context;
    if source != KubeconfigSource::Pasted {
        kubernetes.pasted_kubeconfig = None;
    } else if inline.is_some() {
        kubernetes.pasted_kubeconfig = Some(Uuid::new_v4());
    }
    if *kubernetes != saved.kubernetes {
        kubernetes.home_quota_enforced = false;
    }
    Ok((candidate, inline))
}

async fn preflight(
    active: &Config,
    runtime: &WorkerRuntimeConfig,
    inline: Option<&str>,
) -> TalosResult<()> {
    let mut config = active.clone();
    config.talos.worker.runtime = Some(runtime.clone());
    config.validate_runtime()?;
    match runtime.kind {
        WorkerRuntimeKind::DockerEngine => {
            connect_docker_worker_runtime(&runtime.docker_engine).await?;
        }
        WorkerRuntimeKind::Kubernetes => {
            connect_kubernetes_worker_runtime(&runtime.kubernetes, &active.talos_paths(), inline)
                .await?;
        }
    }
    Ok(())
}

/// Check the deployment-mounted Docker Engine used by a Server or candidate.
pub async fn connect_docker_worker_runtime(
    options: &DockerEngineConfig,
) -> TalosResult<DockerEngine> {
    let unavailable = |error| {
        TalosError::from_source(
            ErrorCode::ServiceUnavailable,
            "The local Docker Engine is unavailable.",
            error,
        )
    };
    let engine = DockerEngine::connect(&options.socket_path, DEFAULT_SECCOMP_PROFILE)
        .map_err(unavailable)?;
    engine.check().await.map_err(unavailable)?;
    Ok(engine)
}

/// Check the explicitly selected Kubernetes API credential and namespace permissions.
pub async fn connect_kubernetes_worker_runtime(
    config: &KubernetesConfig,
    paths: &TalosPaths,
    inline: Option<&str>,
) -> TalosResult<Kubernetes> {
    tokio::time::timeout(PREFLIGHT_TIMEOUT, async {
        let options = KubernetesOptions {
            namespace: config.namespace.clone(),
            storage_class: config.storage_class.clone(),
            seccomp_profile: config.seccomp_profile.clone(),
        };
        let runtime = match config.credential(paths)? {
            KubernetesCredential::InCluster => Kubernetes::in_cluster(options),
            KubernetesCredential::Mounted { path, context } => {
                Kubernetes::from_kubeconfig(options, &path, context.as_deref()).await
            }
            KubernetesCredential::Pasted { path, context } => {
                let saved;
                let contents = if let Some(inline) = inline {
                    inline
                } else {
                    saved = read_pasted_kubeconfig(&path)?;
                    &saved
                };
                Kubernetes::from_inline_kubeconfig(options, contents, context.as_deref()).await
            }
        }
        .map_err(|error| {
            let code = if matches!(error, RuntimeError::InvalidSpec(_)) {
                ErrorCode::InvalidRequest
            } else {
                ErrorCode::ServiceUnavailable
            };
            TalosError::from_source(code, "The Kubernetes runtime credential or target configuration is invalid.", error)
        })?;
        runtime.check(KUBERNETES_HOME_BYTES).await.map_err(|error| {
            TalosError::from_source(
                ErrorCode::ServiceUnavailable,
                "Kubernetes connectivity or the required namespace permissions could not be verified.",
                error,
            )
        })?;
        Ok(runtime)
    })
    .await
    .map_err(|_| TalosError::new(ErrorCode::Timeout, "The Kubernetes runtime preflight timed out."))?
}
