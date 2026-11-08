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
    pub image: String,
    pub control_plane_endpoint: String,
    #[serde(default)]
    pub trusted_ca_file: Option<PathBuf>,
    #[serde(default)]
    pub docker_engine: DockerEngineConfig,
    #[serde(default)]
    pub kubernetes: KubernetesConfig,
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
}

impl Default for DockerEngineConfig {
    fn default() -> Self {
        Self {
            socket_path: "/var/run/docker.sock".into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerRuntimeSelection {
    pub runtime: ServerRuntimePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerRuntimePolicy {
    pub idle_timeout_seconds: u32,
}

impl Default for ServerRuntimePolicy {
    fn default() -> Self {
        Self {
            idle_timeout_seconds: 900,
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
        if self.server.runtime.idle_timeout_seconds == 0 {
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
        if !runtime
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
        let origin = url::Url::parse(&runtime.control_plane_endpoint)
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
        if runtime
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
