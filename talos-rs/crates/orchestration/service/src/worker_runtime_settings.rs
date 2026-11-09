use std::{path::Path, time::Duration};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use rustls::{
    pki_types::{pem::PemObject, CertificateDer},
    RootCertStore,
};

use talos_auth::UserRole;
use talos_config::domains::managed_worker::{
    read_pasted_kubeconfig, write_pasted_kubeconfig, DockerEngineConfig, KubeconfigSource,
    KubernetesConfig, KubernetesCredential, WorkerConnectionConfig, WorkerRuntimeKind,
};
use talos_config::{Config, ServerRuntimePolicy, TalosPaths, WorkerRuntimeConfig, YamlConfigStore};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    WorkerKubeconfigAuthentication, WorkerKubeconfigSource, WorkerKubernetesSettings,
    WorkerRuntimeBackend, WorkerRuntimeCandidate, WorkerRuntimeConnection, WorkerRuntimeProfile,
    WorkerRuntimeSettingsResponse, WorkerRuntimeTiming,
};
use talos_worker_runtime::{
    DockerEngine, Kubernetes, KubernetesOptions, RuntimeError, DEFAULT_SECCOMP_PROFILE,
};
use uuid::Uuid;

use crate::{Actor, TalosService};

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
            active.reload()?
        } else {
            active.clone()
        };
        Ok(settings(active, &saved))
    }

    /// Check a candidate's configuration, connectivity and permissions without saving it.
    pub async fn validate_worker_runtime_settings(
        &self,
        actor: &Actor,
        active: &Config,
        request: WorkerRuntimeCandidate,
    ) -> TalosResult<()> {
        require_admin(actor)?;
        let (candidate, inline) = candidate(load_saved(active)?, request)?;
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
                let (candidate, inline) = candidate(saved.clone(), request)?;
                let target_changed = runtime_changed(
                    active.talos.worker.runtime.as_ref(),
                    candidate.talos.worker.runtime.as_ref(),
                );
                let response = settings(active, &candidate);
                if target_changed && has_instances {
                    return Err(TalosError::new(
                        ErrorCode::Conflict,
                        "The runtime target cannot change while managed Talos still own resources.",
                    ));
                }
                preflight(active, &candidate, inline.as_deref()).await?;
                let runtime = configured_runtime(&candidate)?;
                let previous = configured_runtime(&saved)?;
                if let Some(contents) = inline {
                    let reference = runtime.kubernetes.pasted_kubeconfig.ok_or_else(|| {
                        TalosError::internal("The runtime credential reference is missing.")
                    })?;
                    write_pasted_kubeconfig(&active.talos_paths(), reference, &contents)?;
                }
                let persisted = if previous != runtime
                    || saved.talos.server.runtime != candidate.talos.server.runtime
                {
                    YamlConfigStore::new(active.source_path()).save_worker_runtime_selection(
                        previous,
                        runtime,
                        &saved.talos.server.runtime,
                        &candidate.talos.server.runtime,
                    )
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
                Ok((Ok(response), target_changed))
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

fn load_saved(active: &Config) -> TalosResult<Config> {
    configured_runtime(active)?;
    active.reload()
}

fn configured_runtime(config: &Config) -> TalosResult<&WorkerRuntimeConfig> {
    let unconfigured = || {
        TalosError::new(
            ErrorCode::ServiceUnavailable,
            "A Server operator must configure the Worker image and control plane before editing runtime settings.",
        )
    };
    config
        .talos
        .worker
        .runtime
        .as_ref()
        .ok_or_else(unconfigured)
}

fn settings(active: &Config, saved: &Config) -> WorkerRuntimeSettingsResponse {
    let runtime = saved.talos.worker.runtime.as_ref();
    let policy = &saved.talos.server.runtime;
    let restart_required = runtime_changed(active.talos.worker.runtime.as_ref(), runtime)
        || (runtime.is_some() && &active.talos.server.runtime != policy);
    let active = active.talos.worker.runtime.as_ref();
    let backend = |config: &WorkerRuntimeConfig| match config.kind {
        WorkerRuntimeKind::DockerEngine => WorkerRuntimeBackend::DockerEngine,
        WorkerRuntimeKind::Kubernetes => WorkerRuntimeBackend::Kubernetes,
    };
    WorkerRuntimeSettingsResponse {
        active_backend: active.map(backend),
        configured_backend: runtime.map(backend),
        profile: runtime.map(profile),
        timing: runtime.map(|_| timing(policy)),
        docker_socket_path: runtime.map(|runtime| {
            runtime
                .docker_engine
                .socket_path
                .to_string_lossy()
                .into_owned()
        }),
        kubernetes: runtime.map(|runtime| {
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
                seccomp_profile: kube.seccomp_profile.clone(),
                home_bytes: kube.home_bytes,
                home_quota_enforced: kube.home_quota_enforced,
            }
        }),
        restart_required,
    }
}

fn profile(runtime: &WorkerRuntimeConfig) -> WorkerRuntimeProfile {
    let worker = runtime.connection(runtime.kind);
    let resources = runtime.resources(runtime.kind);
    WorkerRuntimeProfile {
        worker: WorkerRuntimeConnection {
            image: worker.image,
            control_plane_endpoint: worker.control_plane_endpoint,
            trusted_ca_file: worker
                .trusted_ca_file
                .map(|path| path.to_string_lossy().into_owned()),
        },
        cpu_cores: resources.cpu_cores,
        memory_bytes: resources.memory_bytes,
        shm_bytes: resources.shm_bytes,
    }
}

fn timing(policy: &ServerRuntimePolicy) -> WorkerRuntimeTiming {
    WorkerRuntimeTiming {
        idle_timeout_seconds: policy.idle_timeout_seconds,
        request_wait_seconds: policy.request_wait_seconds,
        start_timeout_seconds: policy.start_timeout_seconds,
        wake_cooldown_seconds: policy.wake_cooldown_seconds,
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
                || active.connection(active.kind) != saved.connection(active.kind)
                || match active.kind {
                    WorkerRuntimeKind::DockerEngine => active.docker_engine != saved.docker_engine,
                    WorkerRuntimeKind::Kubernetes => active.kubernetes != saved.kubernetes,
                }
        }
        _ => true,
    }
}

fn candidate(
    mut config: Config,
    request: WorkerRuntimeCandidate,
) -> TalosResult<(Config, Option<String>)> {
    let saved = configured_runtime(&config)?;
    let mut runtime = saved.clone();
    let (edits, inline) = match (runtime.kind, request) {
        (WorkerRuntimeKind::DockerEngine, WorkerRuntimeCandidate::DockerEngine { edits }) => {
            (edits, None)
        }
        (
            WorkerRuntimeKind::Kubernetes,
            WorkerRuntimeCandidate::Kubernetes {
                namespace,
                storage_class,
                authentication,
                edits,
            },
        ) => {
            let inline = edit_kubernetes(
                &mut runtime.kubernetes,
                namespace,
                storage_class,
                authentication,
            )?;
            if runtime.kubernetes != saved.kubernetes {
                runtime.kubernetes.home_quota_enforced = false;
            }
            (edits, inline)
        }
        _ => {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "The Worker runtime kind is set by the Server deployment.",
            ))
        }
    };
    if let Some(edits) = edits {
        if let Some(idle_timeout_seconds) = edits.idle_timeout_seconds {
            config.talos.server.runtime.idle_timeout_seconds = idle_timeout_seconds;
        }
        if let Some(worker) = edits.worker {
            let connection = WorkerConnectionConfig {
                image: worker.image.trim().into(),
                control_plane_endpoint: worker.control_plane_endpoint.trim().into(),
                trusted_ca_file: worker.trusted_ca_file.map(Into::into),
            };
            if connection != runtime.connection(runtime.kind) {
                match runtime.kind {
                    WorkerRuntimeKind::DockerEngine => {
                        runtime.docker_engine.worker = Some(connection)
                    }
                    WorkerRuntimeKind::Kubernetes => runtime.kubernetes.worker = Some(connection),
                }
            }
        }
    }
    config.talos.worker.runtime = Some(runtime);
    Ok((config, inline))
}

fn edit_kubernetes(
    kubernetes: &mut KubernetesConfig,
    namespace: String,
    storage_class: String,
    authentication: WorkerKubeconfigAuthentication,
) -> TalosResult<Option<String>> {
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
    Ok(inline)
}

async fn preflight(active: &Config, candidate: &Config, inline: Option<&str>) -> TalosResult<()> {
    let mut config = active.clone();
    config.talos = candidate.talos.clone();
    config.validate_runtime()?;
    let runtime = configured_runtime(candidate)?;
    read_worker_trusted_ca(runtime.connection(runtime.kind).trusted_ca_file.as_deref())?;
    match runtime.kind {
        WorkerRuntimeKind::DockerEngine => {
            connect_docker_worker_runtime(&runtime.docker_engine).await?;
        }
        WorkerRuntimeKind::Kubernetes => {
            connect_kubernetes_worker_runtime(&runtime.kubernetes, &config.talos_paths(), inline)
                .await?;
        }
    }
    Ok(())
}

/// Read only validated certificates, so an editable path cannot copy other file contents into Worker Home.
pub fn read_worker_trusted_ca(path: Option<&Path>) -> TalosResult<Option<Vec<u8>>> {
    let Some(path) = path else { return Ok(None) };
    let invalid = || {
        TalosError::new(
            ErrorCode::InvalidRequest,
            "The Worker CA file must contain usable PEM certificates.",
        )
    };
    let read_error = |error| {
        TalosError::from_source(
            ErrorCode::InvalidRequest,
            "Cannot read the Worker CA file.",
            error,
        )
    };
    if !path.metadata().map_err(read_error)?.is_file() {
        return Err(invalid());
    }
    let file = std::fs::File::open(path).map_err(read_error)?;
    let mut roots = RootCertStore::empty();
    let mut pem = String::new();
    for certificate in CertificateDer::pem_reader_iter(file) {
        let certificate = certificate.map_err(|_| invalid())?;
        if roots.add(certificate.clone()).is_err() {
            continue;
        }
        pem.push_str("-----BEGIN CERTIFICATE-----\n");
        for line in STANDARD.encode(&certificate).as_bytes().chunks(64) {
            pem.extend(line.iter().copied().map(char::from));
            pem.push('\n');
        }
        pem.push_str("-----END CERTIFICATE-----\n");
    }
    if roots.is_empty() {
        return Err(invalid());
    }
    Ok(Some(pem.into_bytes()))
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
        runtime.check(config.home_bytes).await.map_err(|error| {
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
