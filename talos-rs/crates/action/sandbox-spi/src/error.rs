use thiserror::Error;

use crate::SandboxIsolationCapability;

/// Describes validation, capability, and preparation failures at the provider boundary.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum SandboxProviderError {
    #[error("sandbox capability is unsupported: {capability:?}: {reason}")]
    UnsupportedCapability {
        capability: SandboxIsolationCapability,
        reason: String,
    },
    #[error("sandbox provider probe failed: {reason}")]
    ProbeFailed { reason: String },
    #[error("invalid sandbox isolation spec: {reason}")]
    InvalidIsolationSpec { reason: String },
    #[error("sandbox command preparation failed: {reason}")]
    PrepareFailed { reason: String },
}
