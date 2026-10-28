use serde::{Deserialize, Serialize};
use talos_types::{PluginInstallRequest, PluginUpdateRequest};

/// Private Worker transport input. Preparation never grants permission to commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkerPluginChange {
    Install(PluginInstallRequest),
    Update(PluginUpdateRequest),
}

/// One-use, process-local candidate reference; never exposed by public Plugin APIs.
#[derive(Debug, Clone)]
pub struct WorkerPluginPrepared {
    pub preparation_id: String,
}

#[derive(Debug, Clone)]
pub struct WorkerPluginInstall {
    pub request: PluginInstallRequest,
    pub preparation_id: String,
}

#[derive(Debug, Clone)]
pub struct WorkerPluginUpdate {
    pub request: PluginUpdateRequest,
    pub preparation_id: String,
}
