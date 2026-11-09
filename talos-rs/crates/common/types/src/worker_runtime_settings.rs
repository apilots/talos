//! Administrator-only configuration for the Server's managed Worker runtime.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkerRuntimeBackend {
    DockerEngine,
    Kubernetes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkerKubeconfigSource {
    InCluster,
    Mounted,
    Pasted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkerKubernetesSettings {
    pub namespace: String,
    pub storage_class: String,
    pub authentication_source: WorkerKubeconfigSource,
    pub mounted_kubeconfig_path: Option<String>,
    pub context: Option<String>,
    pub seccomp_profile: String,
    #[ts(type = "number")]
    pub home_bytes: u64,
    pub home_quota_enforced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRuntimeConnection {
    pub image: String,
    pub control_plane_endpoint: String,
    pub trusted_ca_file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRuntimeProfile {
    pub worker: WorkerRuntimeConnection,
    pub cpu_cores: u16,
    #[ts(type = "number")]
    pub memory_bytes: u64,
    #[ts(type = "number")]
    pub shm_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRuntimeTiming {
    pub idle_timeout_seconds: u32,
    pub request_wait_seconds: u32,
    pub start_timeout_seconds: u32,
    pub wake_cooldown_seconds: u32,
}

/// Effective backend and saved settings. The Server never returns pasted credentials or their references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRuntimeSettingsResponse {
    pub active_backend: Option<WorkerRuntimeBackend>,
    pub configured_backend: Option<WorkerRuntimeBackend>,
    pub docker_socket_path: Option<String>,
    pub profile: Option<WorkerRuntimeProfile>,
    pub timing: Option<WorkerRuntimeTiming>,
    pub kubernetes: Option<WorkerKubernetesSettings>,
    pub restart_required: bool,
}

/// Edits to the deployed backend and the shared Server policy.
#[derive(Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerRuntimeEdits {
    /// Omitted or null preserves the existing connection. A null CA uses system trust.
    #[ts(optional = nullable)]
    pub worker: Option<WorkerRuntimeConnection>,
    /// Omitted or null preserves the common idle timeout.
    #[ts(optional = nullable)]
    pub idle_timeout_seconds: Option<u32>,
}

/// The kind must match the Server deployment.
#[derive(Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
#[ts(tag = "kind", rename_all = "camelCase")]
pub enum WorkerRuntimeCandidate {
    DockerEngine {
        #[serde(default)]
        #[ts(optional)]
        edits: Option<WorkerRuntimeEdits>,
    },
    Kubernetes {
        namespace: String,
        #[serde(rename = "storageClass")]
        storage_class: String,
        authentication: WorkerKubeconfigAuthentication,
        #[serde(default)]
        #[ts(optional)]
        edits: Option<WorkerRuntimeEdits>,
    },
}

#[derive(Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
#[ts(tag = "kind", rename_all = "camelCase")]
pub enum WorkerKubeconfigAuthentication {
    InCluster,
    Mounted {
        /// Omitted or null uses the kubeconfig's current context.
        #[ts(optional = nullable)]
        context: Option<String>,
    },
    Pasted {
        /// Omitted or null uses the kubeconfig's current context.
        #[ts(optional = nullable)]
        context: Option<String>,
        /// Omitted or null keeps the Server's saved pasted credential. Write only.
        #[ts(optional = nullable)]
        kubeconfig: Option<String>,
    },
}
