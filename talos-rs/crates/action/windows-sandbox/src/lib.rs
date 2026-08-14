//! Reserves the Windows provider boundary for the Talos sandbox SPI.
//!
//! The initial implementation will report enforcing execution as unsupported.
//! Windows isolation mechanics remain out of scope and this crate must not
//! interpret Talos permission modes or define duplicate public sandbox objects.

use talos_sandbox_spi::{
    FileSystemIsolationCapabilities, NetworkIsolationCapabilities, PreparedSandboxCommand,
    SandboxExecutionRequest, SandboxIsolationCapability, SandboxProvider,
    SandboxProviderCapabilities, SandboxProviderError, SandboxProviderPlatform,
    SandboxProviderSnapshot, SandboxRequestSupport,
};

pub const WINDOWS_SANDBOX_PROVIDER_NAME: &str = "windows-placeholder";

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsSandboxProvider;

impl WindowsSandboxProvider {
    pub fn new() -> Self {
        Self
    }
}

impl SandboxProvider for WindowsSandboxProvider {
    fn name(&self) -> &'static str {
        WINDOWS_SANDBOX_PROVIDER_NAME
    }

    fn supports_platform(&self, platform: SandboxProviderPlatform) -> bool {
        matches!(
            platform,
            SandboxProviderPlatform::WindowsRestrictedToken
                | SandboxProviderPlatform::WindowsElevated
        )
    }

    fn probe(
        &self,
        platform: SandboxProviderPlatform,
    ) -> Result<SandboxProviderCapabilities, SandboxProviderError> {
        if !self.supports_platform(platform) {
            return Err(SandboxProviderError::ProbeFailed {
                reason: "Windows provider received a non-Windows platform".to_string(),
            });
        }
        Ok(SandboxProviderCapabilities {
            platform,
            file_system: FileSystemIsolationCapabilities {
                read: false,
                write: false,
                deny_read: false,
            },
            network: NetworkIsolationCapabilities { restricted: false },
            private_tmp: false,
            helper_required: false,
        })
    }

    fn support(
        &self,
        request: &SandboxExecutionRequest,
        _snapshot: &SandboxProviderSnapshot,
    ) -> SandboxRequestSupport {
        SandboxRequestSupport::Unsupported {
            capability: SandboxIsolationCapability::FileSystemRead,
            reason: format!(
                "Windows sandbox provider is not implemented for {:?}",
                request.platform()
            ),
        }
    }

    fn prepare(
        &self,
        request: SandboxExecutionRequest,
    ) -> Result<PreparedSandboxCommand, SandboxProviderError> {
        Err(SandboxProviderError::UnsupportedCapability {
            capability: SandboxIsolationCapability::FileSystemRead,
            reason: format!(
                "Windows sandbox provider is not implemented for {:?}",
                request.platform()
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use talos_sandbox_spi::{
        provider_contract, ProviderContractExpectation, SandboxIsolationCapability,
        SandboxProviderPlatform,
    };

    use super::*;

    #[test]
    fn placeholder_satisfies_both_unsupported_provider_contracts() {
        for platform in [
            SandboxProviderPlatform::WindowsRestrictedToken,
            SandboxProviderPlatform::WindowsElevated,
        ] {
            provider_contract::assert_provider_contract(
                &WindowsSandboxProvider::new(),
                platform,
                ProviderContractExpectation::Unsupported {
                    capability: SandboxIsolationCapability::FileSystemRead,
                },
            );
        }
    }
}
