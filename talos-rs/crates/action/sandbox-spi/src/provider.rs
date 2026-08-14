use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    NetworkIsolationSpec, SandboxExecutionCommand, SandboxExecutionRequest, SandboxProviderError,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SandboxProviderPlatform {
    MacosSeatbelt,
    LinuxBubblewrap,
    WindowsRestrictedToken,
    WindowsElevated,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SandboxIsolationCapability {
    FileSystemRead,
    FileSystemWrite,
    DenyRead,
    NetworkRestricted,
    PrivateTmp,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileSystemIsolationCapabilities {
    pub read: bool,
    pub write: bool,
    pub deny_read: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkIsolationCapabilities {
    pub restricted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SandboxProviderCapabilities {
    pub platform: SandboxProviderPlatform,
    pub file_system: FileSystemIsolationCapabilities,
    pub network: NetworkIsolationCapabilities,
    pub private_tmp: bool,
    pub helper_required: bool,
}

impl SandboxProviderCapabilities {
    pub fn all(platform: SandboxProviderPlatform, helper_required: bool) -> Self {
        Self {
            platform,
            file_system: FileSystemIsolationCapabilities {
                read: true,
                write: true,
                deny_read: true,
            },
            network: NetworkIsolationCapabilities { restricted: true },
            private_tmp: true,
            helper_required,
        }
    }

    fn supported_capabilities(&self) -> BTreeSet<SandboxIsolationCapability> {
        let mut capabilities = BTreeSet::new();
        if self.file_system.read {
            capabilities.insert(SandboxIsolationCapability::FileSystemRead);
        }
        if self.file_system.write {
            capabilities.insert(SandboxIsolationCapability::FileSystemWrite);
        }
        if self.file_system.deny_read {
            capabilities.insert(SandboxIsolationCapability::DenyRead);
        }
        if self.network.restricted {
            capabilities.insert(SandboxIsolationCapability::NetworkRestricted);
        }
        if self.private_tmp {
            capabilities.insert(SandboxIsolationCapability::PrivateTmp);
        }
        capabilities
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SandboxProviderSnapshot {
    Available {
        capabilities: SandboxProviderCapabilities,
    },
    Failed {
        reason: String,
    },
}

impl From<SandboxProviderCapabilities> for SandboxProviderSnapshot {
    fn from(capabilities: SandboxProviderCapabilities) -> Self {
        Self::Available { capabilities }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SandboxRequestSupport {
    Supported,
    Unsupported {
        capability: SandboxIsolationCapability,
        reason: String,
    },
    Unavailable {
        reason: String,
    },
}

impl SandboxRequestSupport {
    pub fn require_supported(&self) -> Result<(), SandboxProviderError> {
        match self {
            Self::Supported => Ok(()),
            Self::Unsupported { capability, reason } => {
                Err(SandboxProviderError::UnsupportedCapability {
                    capability: *capability,
                    reason: reason.clone(),
                })
            }
            Self::Unavailable { reason } => Err(SandboxProviderError::ProbeFailed {
                reason: reason.clone(),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedSandboxCommand {
    command: SandboxExecutionCommand,
}

impl PreparedSandboxCommand {
    pub fn from_command(command: SandboxExecutionCommand) -> Self {
        Self { command }
    }

    pub fn command(&self) -> &SandboxExecutionCommand {
        &self.command
    }

    pub fn into_command(self) -> SandboxExecutionCommand {
        self.command
    }
}

/// Defines the contract implemented by each OS sandbox provider.
///
/// Implementations only translate validated SPI requests into platform launch
/// commands. They must not interpret Talos permissions or select a fallback.
pub trait SandboxProvider: Send + Sync {
    fn name(&self) -> &'static str;

    fn supports_platform(&self, platform: SandboxProviderPlatform) -> bool;

    fn probe(
        &self,
        platform: SandboxProviderPlatform,
    ) -> Result<SandboxProviderCapabilities, SandboxProviderError>;

    fn support(
        &self,
        request: &SandboxExecutionRequest,
        snapshot: &SandboxProviderSnapshot,
    ) -> SandboxRequestSupport;

    fn prepare(
        &self,
        request: SandboxExecutionRequest,
    ) -> Result<PreparedSandboxCommand, SandboxProviderError>;
}

pub fn default_request_support(
    request: &SandboxExecutionRequest,
    snapshot: &SandboxProviderSnapshot,
) -> SandboxRequestSupport {
    let capabilities = match snapshot {
        SandboxProviderSnapshot::Available { capabilities } => capabilities,
        SandboxProviderSnapshot::Failed { reason } => {
            return SandboxRequestSupport::Unavailable {
                reason: reason.clone(),
            };
        }
    };
    if request.platform() != capabilities.platform {
        return SandboxRequestSupport::Unavailable {
            reason: "provider snapshot platform does not match request".to_string(),
        };
    }

    let supported = capabilities.supported_capabilities();
    for required in required_capabilities(request) {
        if !supported.contains(&required) {
            return SandboxRequestSupport::Unsupported {
                capability: required,
                reason: format!("provider does not support {required:?}"),
            };
        }
    }
    SandboxRequestSupport::Supported
}

fn required_capabilities(
    request: &SandboxExecutionRequest,
) -> BTreeSet<SandboxIsolationCapability> {
    let mut required = BTreeSet::new();
    if !request.file_system().readable_roots().is_empty() {
        required.insert(SandboxIsolationCapability::FileSystemRead);
    }
    if !request.file_system().writable_roots().is_empty() {
        required.insert(SandboxIsolationCapability::FileSystemWrite);
    }
    if !request.file_system().denied_roots().is_empty() {
        required.insert(SandboxIsolationCapability::DenyRead);
    }
    if request.network() == NetworkIsolationSpec::Restricted {
        required.insert(SandboxIsolationCapability::NetworkRestricted);
    }
    if request.runtime().private_tmp {
        required.insert(SandboxIsolationCapability::PrivateTmp);
    }
    required
}
