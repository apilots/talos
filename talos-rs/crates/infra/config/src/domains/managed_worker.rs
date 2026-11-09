use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use talos_error::{ErrorCode, TalosError, TalosResult};

use super::agent_node::{AgentNodeConfig, AgentNodeRunnerConfig};
use super::network::{GatewayConfig, LlmConfig};
use super::sync::SyncConfig;
use crate::{Config, ModelRef, WorkspaceConfig};

#[path = "kubernetes_worker.rs"]
mod kubernetes;
pub use kubernetes::{
    read_pasted_kubeconfig, write_pasted_kubeconfig, KubeconfigSource, KubernetesConfig,
    KubernetesCredential,
};

/// Runtime configuration is opt-in. An ordinary Server or Worker remains unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TalosRuntimeConfig {
    pub worker: WorkerRuntimeSelection,
    pub server: ServerRuntimeSelection,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkerRuntimeSelection {
    pub runtime: Option<WorkerRuntimeConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRuntimeConfig {
    pub kind: WorkerRuntimeKind,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub control_plane_endpoint: String,
    #[serde(default)]
    pub trusted_ca_file: Option<PathBuf>,
    #[serde(default)]
    pub docker_engine: DockerEngineConfig,
    #[serde(default)]
    pub kubernetes: KubernetesConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerConnectionConfig {
    pub image: String,
    pub control_plane_endpoint: String,
    #[serde(default)]
    pub trusted_ca_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkerResources {
    pub cpu_cores: u16,
    pub memory_bytes: u64,
    pub shm_bytes: u64,
}

impl Default for WorkerResources {
    fn default() -> Self {
        Self {
            cpu_cores: 4,
            memory_bytes: 8 << 30,
            shm_bytes: 1 << 30,
        }
    }
}

impl WorkerRuntimeConfig {
    pub fn connection_override(&self, kind: WorkerRuntimeKind) -> &Option<WorkerConnectionConfig> {
        match kind {
            WorkerRuntimeKind::DockerEngine => &self.docker_engine.worker,
            WorkerRuntimeKind::Kubernetes => &self.kubernetes.worker,
        }
    }

    pub fn connection(&self, kind: WorkerRuntimeKind) -> WorkerConnectionConfig {
        self.connection_override(kind)
            .clone()
            .unwrap_or_else(|| WorkerConnectionConfig {
                image: self.image.clone(),
                control_plane_endpoint: self.control_plane_endpoint.clone(),
                trusted_ca_file: self.trusted_ca_file.clone(),
            })
    }

    pub fn resources(&self, kind: WorkerRuntimeKind) -> WorkerResources {
        match kind {
            WorkerRuntimeKind::DockerEngine => self.docker_engine.resources,
            WorkerRuntimeKind::Kubernetes => self.kubernetes.resources,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerRuntimeKind {
    DockerEngine,
    Kubernetes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DockerEngineConfig {
    pub socket_path: PathBuf,
    pub worker: Option<WorkerConnectionConfig>,
    pub resources: WorkerResources,
}

impl Default for DockerEngineConfig {
    fn default() -> Self {
        Self {
            socket_path: "/var/run/docker.sock".into(),
            worker: None,
            resources: WorkerResources::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerRuntimeSelection {
    pub runtime: ServerRuntimePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerRuntimePolicy {
    pub idle_timeout_seconds: u32,
    pub request_wait_seconds: u32,
    pub start_timeout_seconds: u32,
    pub wake_cooldown_seconds: u32,
}

impl Default for ServerRuntimePolicy {
    fn default() -> Self {
        Self {
            idle_timeout_seconds: 900,
            request_wait_seconds: 45,
            start_timeout_seconds: 120,
            wake_cooldown_seconds: 60,
        }
    }
}

impl TalosRuntimeConfig {
    pub(crate) fn validate(&self, config: &Config) -> TalosResult<()> {
        let Some(runtime) = &self.worker.runtime else {
            return Ok(());
        };
        let invalid = |message| TalosError::new(ErrorCode::InvalidRequest, message);
        if !config.gateway.auth || config.agent_node.control_plane.is_none() {
            return Err(invalid("The managed Worker runtime requires an authenticated Server with an Agent node control plane."));
        }
        let policy = &self.server.runtime;
        if policy.request_wait_seconds == 0
            || policy.request_wait_seconds > policy.start_timeout_seconds
            || policy.wake_cooldown_seconds == 0
        {
            return Err(invalid("Runtime timing must be positive and the request wait must not exceed the start timeout."));
        }
        if policy.idle_timeout_seconds == 0 {
            return Err(invalid(
                "talos.server.runtime.idle_timeout_seconds must be positive.",
            ));
        }
        match runtime.kind {
            WorkerRuntimeKind::DockerEngine => {
                if !runtime.docker_engine.socket_path.is_absolute() {
                    return Err(invalid(
                        "The Docker Engine socket must be an absolute Unix socket path.",
                    ));
                }
            }
            WorkerRuntimeKind::Kubernetes => {
                runtime.kubernetes.validate(&config.talos_paths())?;
            }
        }
        runtime.resources(runtime.kind).validate()?;
        runtime.connection(runtime.kind).validate()?;
        Ok(())
    }
}

impl WorkerResources {
    fn validate(self) -> TalosResult<()> {
        if self.cpu_cores == 0
            || self.memory_bytes == 0
            || self.memory_bytes > i64::MAX as u64
            || self.shm_bytes == 0
            || self.shm_bytes > self.memory_bytes
        {
            return Err(TalosError::new(ErrorCode::InvalidRequest, "Worker CPU, memory and shared memory must be positive. Shared memory cannot exceed Worker memory."));
        }
        Ok(())
    }
}

impl WorkerConnectionConfig {
    fn validate(&self) -> TalosResult<()> {
        let invalid = |message| TalosError::new(ErrorCode::InvalidRequest, message);
        if !self
            .image
            .rsplit_once("@sha256:")
            .is_some_and(|(repository, digest)| {
                !repository.trim().is_empty()
                    && digest.len() == 64
                    && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            return Err(invalid(
                "The managed Worker image must be pinned by SHA-256 digest.",
            ));
        }
        let origin = url::Url::parse(&self.control_plane_endpoint)
            .map_err(|_| invalid("The managed Worker control plane must be an HTTPS origin."))?;
        if origin.scheme() != "https"
            || origin.host().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
        {
            return Err(invalid(
                "The managed Worker control plane must be an HTTPS origin.",
            ));
        }
        if self
            .trusted_ca_file
            .as_ref()
            .is_some_and(|path| path.as_os_str().is_empty())
        {
            return Err(invalid("The managed Worker trusted CA path is empty."));
        }
        Ok(())
    }
}

/// Render the existing outbound Runner config with only the required shared loader
/// sections. Provider setup and all future edits are local to the Worker Home.
pub fn managed_worker_seed_config(runner: AgentNodeRunnerConfig) -> TalosResult<Vec<u8>> {
    #[derive(Serialize)]
    struct AgentSeed {
        name: &'static str,
        inner_model: Option<ModelRef>,
    }

    #[derive(Serialize)]
    struct WorkerSeed {
        workspaces: WorkspaceConfig,
        agent: AgentSeed,
        gateway: GatewayConfig,
        llm: LlmConfig,
        sync: SyncConfig,
        agent_node: AgentNodeConfig,
    }

    serde_yaml::to_string(&WorkerSeed {
        workspaces: WorkspaceConfig::default(),
        agent: AgentSeed {
            name: "talos-worker",
            // Override the packaged default until this Worker has its own model.
            inner_model: None,
        },
        gateway: GatewayConfig::default(),
        llm: LlmConfig::default(),
        sync: SyncConfig {
            enabled: Some(false),
            ..Default::default()
        },
        agent_node: AgentNodeConfig {
            runner: Some(runner),
            ..Default::default()
        },
    })
    .map(String::into_bytes)
    .map_err(|error| {
        TalosError::internal_with_source("Serialize managed Worker bootstrap configuration.", error)
    })
}
