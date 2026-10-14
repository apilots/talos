//! Implements the macOS sandbox provider for the Talos sandbox SPI.
//!
//! This crate owns Seatbelt-specific translation and execution details. It must
//! not interpret Talos permission modes or expose a second sandbox data model.

mod profile;

use std::path::PathBuf;

use talos_sandbox_spi::{
    default_request_support, PreparedSandboxCommand, SandboxExecutionCommand,
    SandboxExecutionRequest, SandboxProvider, SandboxProviderCapabilities, SandboxProviderError,
    SandboxProviderPlatform, SandboxProviderSnapshot, SandboxRequestSupport,
};

use profile::build_seatbelt_profile;

pub const MACOS_SANDBOX_PROVIDER_NAME: &str = "macos-seatbelt";
pub const MACOS_SANDBOX_EXECUTABLE: &str = "/usr/bin/sandbox-exec";

#[derive(Clone, Debug)]
pub struct MacosSandboxProvider {
    sandbox_executable: talos_sandbox_spi::AbsolutePathBuf,
}

impl MacosSandboxProvider {
    pub fn new() -> Self {
        Self {
            sandbox_executable: talos_sandbox_spi::AbsolutePathBuf::new(PathBuf::from(
                MACOS_SANDBOX_EXECUTABLE,
            ))
            .expect("the macOS sandbox executable path must be absolute"),
        }
    }

    /// Overrides the trusted Seatbelt executable selected by the runtime assembly layer.
    pub fn with_executable(executable: PathBuf) -> Result<Self, SandboxProviderError> {
        Ok(Self {
            sandbox_executable: talos_sandbox_spi::AbsolutePathBuf::new(executable)?,
        })
    }
}

impl Default for MacosSandboxProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SandboxProvider for MacosSandboxProvider {
    fn name(&self) -> &'static str {
        MACOS_SANDBOX_PROVIDER_NAME
    }

    fn supports_platform(&self, platform: SandboxProviderPlatform) -> bool {
        platform == SandboxProviderPlatform::MacosSeatbelt
    }

    fn probe(
        &self,
        platform: SandboxProviderPlatform,
    ) -> Result<SandboxProviderCapabilities, SandboxProviderError> {
        if !self.supports_platform(platform) {
            return Err(SandboxProviderError::ProbeFailed {
                reason: "Seatbelt provider received a non-macOS platform".to_string(),
            });
        }
        #[cfg(not(target_os = "macos"))]
        return Err(SandboxProviderError::ProbeFailed {
            reason: "Seatbelt sandbox is only available on macOS".to_string(),
        });
        #[cfg(target_os = "macos")]
        {
            let executable_path = self.sandbox_executable.as_path();
            let executable = std::fs::metadata(executable_path).map_err(|error| {
                SandboxProviderError::ProbeFailed {
                    reason: format!("{} is unavailable: {error}", executable_path.display()),
                }
            })?;
            if !executable.is_file() {
                return Err(SandboxProviderError::ProbeFailed {
                    reason: format!("{} is not a file", executable_path.display()),
                });
            }
            Ok(SandboxProviderCapabilities::all(platform, false))
        }
    }

    fn support(
        &self,
        request: &SandboxExecutionRequest,
        snapshot: &SandboxProviderSnapshot,
    ) -> SandboxRequestSupport {
        default_request_support(request, snapshot)
    }

    fn prepare(
        &self,
        request: SandboxExecutionRequest,
    ) -> Result<PreparedSandboxCommand, SandboxProviderError> {
        if request.platform() != SandboxProviderPlatform::MacosSeatbelt {
            return Err(SandboxProviderError::PrepareFailed {
                reason: "Seatbelt provider received a non-macOS request".to_string(),
            });
        }
        let host_tmp = std::env::var_os("TMPDIR").map(PathBuf::from);
        let (policy, definitions) = build_seatbelt_profile(&request, host_tmp.as_deref());
        let original = request.into_command();
        let mut args = vec!["-p".to_string(), policy];
        args.extend(
            definitions
                .into_iter()
                .map(|(name, value)| format!("-D{name}={}", value.display())),
        );
        args.push("--".to_string());
        args.push(original.program().as_path().to_string_lossy().into_owned());
        args.extend(original.args().iter().cloned());

        let command = SandboxExecutionCommand::new(
            self.sandbox_executable.clone(),
            args,
            original.env().clone(),
            original.cwd().clone(),
            original.stdin().clone(),
        )?;
        Ok(PreparedSandboxCommand::from_command(command))
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
