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
}

/// Effective backend and saved settings. The Server never returns pasted credentials or their references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRuntimeSettingsResponse {
    pub active_backend: Option<WorkerRuntimeBackend>,
    pub configured_backend: Option<WorkerRuntimeBackend>,
    pub docker_socket_path: Option<String>,
    pub kubernetes: Option<WorkerKubernetesSettings>,
    pub restart_required: bool,
}

/// A candidate can select a deployed backend and change its namespace, class and authentication.
#[derive(Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
#[ts(tag = "kind", rename_all = "camelCase")]
pub enum WorkerRuntimeCandidate {
    DockerEngine,
    Kubernetes {
        namespace: String,
        #[serde(rename = "storageClass")]
        storage_class: String,
        authentication: WorkerKubeconfigAuthentication,
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
