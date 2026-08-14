//! Defines the workspace-internal contract implemented by OS sandbox providers.
//!
//! This crate owns normalized execution data and provider interfaces. It must not
//! contain Talos tool permissions, approval policy, runtime orchestration, or OS
//! implementation details.

mod error;
mod path;
mod provider;
mod request;

pub use error::SandboxProviderError;
pub use path::AbsolutePathBuf;
pub use provider::{
    default_request_support, FileSystemIsolationCapabilities, NetworkIsolationCapabilities,
    PreparedSandboxCommand, SandboxIsolationCapability, SandboxProvider,
    SandboxProviderCapabilities, SandboxProviderPlatform, SandboxProviderSnapshot,
    SandboxRequestSupport,
};
pub use request::{
    FileSystemIsolationSpec, NetworkIsolationSpec, SandboxExecutionCommand,
    SandboxExecutionRequest, SandboxPathAccess, SandboxRuntimeRequirements, SandboxStdin,
    MAX_ARGUMENT_BYTES, MAX_ARGUMENT_COUNT, MAX_ENVIRONMENT_BYTES, MAX_ENVIRONMENT_ENTRIES,
    MAX_ENVIRONMENT_KEY_BYTES, MAX_ENVIRONMENT_VALUE_BYTES, MAX_INLINE_STDIN_BYTES, MAX_ROOT_COUNT,
};

#[cfg(any(test, feature = "provider-contract-tests"))]
pub mod provider_contract;

#[cfg(any(test, feature = "provider-contract-tests"))]
pub use provider_contract::ProviderContractExpectation;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
