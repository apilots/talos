//! Implements the Linux sandbox provider for the Talos sandbox SPI.
//!
//! This crate owns bubblewrap, seccomp, and launcher protocol details. It must
//! not interpret Talos permission modes or expose a second sandbox data model.

mod protocol;

#[cfg(target_os = "linux")]
mod launcher;
#[cfg(target_os = "linux")]
mod seccomp;

use std::collections::BTreeMap;
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::{path::Path, process::Command};

use talos_sandbox_spi::{
    default_request_support, AbsolutePathBuf, PreparedSandboxCommand, SandboxExecutionCommand,
    SandboxExecutionRequest, SandboxProvider, SandboxProviderCapabilities, SandboxProviderError,
    SandboxProviderPlatform, SandboxProviderSnapshot, SandboxRequestSupport, SandboxStdin,
};

use protocol::{LauncherRequest, PROTOCOL_VERSION};

pub const LINUX_SANDBOX_PROVIDER_NAME: &str = "linux-bubblewrap";

/// Configures trusted executables needed by the Linux provider.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxSandboxProviderConfig {
    helper_executable: AbsolutePathBuf,
    bubblewrap_executable: AbsolutePathBuf,
}

impl LinuxSandboxProviderConfig {
    pub fn new(helper_executable: PathBuf) -> Result<Self, SandboxProviderError> {
        let bubblewrap_executable = ["/usr/bin/bwrap", "/bin/bwrap"]
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.is_file())
            .unwrap_or_else(|| PathBuf::from("/usr/bin/bwrap"));
        Ok(Self {
            helper_executable: AbsolutePathBuf::new(helper_executable)?,
            bubblewrap_executable: AbsolutePathBuf::new(bubblewrap_executable)?,
        })
    }

    pub fn with_bubblewrap(
        helper_executable: PathBuf,
        bubblewrap_executable: PathBuf,
    ) -> Result<Self, SandboxProviderError> {
        Ok(Self {
            helper_executable: AbsolutePathBuf::new(helper_executable)?,
            bubblewrap_executable: AbsolutePathBuf::new(bubblewrap_executable)?,
        })
    }
}

#[derive(Clone, Debug)]
pub struct LinuxSandboxProvider {
    config: LinuxSandboxProviderConfig,
}

impl LinuxSandboxProvider {
    pub fn new(config: LinuxSandboxProviderConfig) -> Self {
        Self { config }
    }
}

impl SandboxProvider for LinuxSandboxProvider {
    fn name(&self) -> &'static str {
        LINUX_SANDBOX_PROVIDER_NAME
    }

    fn supports_platform(&self, platform: SandboxProviderPlatform) -> bool {
        platform == SandboxProviderPlatform::LinuxBubblewrap
    }

    fn probe(
        &self,
        platform: SandboxProviderPlatform,
    ) -> Result<SandboxProviderCapabilities, SandboxProviderError> {
        if !self.supports_platform(platform) {
            return probe_failed("bubblewrap provider received a non-Linux platform");
        }
        #[cfg(not(target_os = "linux"))]
        return probe_failed("bubblewrap sandbox is only available on Linux");
        #[cfg(target_os = "linux")]
        {
            require_file(self.config.helper_executable.as_path(), "launcher helper")?;
            require_file(
                self.config.bubblewrap_executable.as_path(),
                "system bubblewrap",
            )?;
            probe_seccomp(self.config.helper_executable.as_path())?;
            probe_bubblewrap(self.config.bubblewrap_executable.as_path())?;
            Ok(SandboxProviderCapabilities::all(platform, true))
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
        if request.platform() != SandboxProviderPlatform::LinuxBubblewrap {
            return prepare_failed("bubblewrap provider received a non-Linux request");
        }
        let launcher_request = LauncherRequest::from_spi(
            PROTOCOL_VERSION,
            &request,
            self.config.bubblewrap_executable.as_path(),
            self.config.helper_executable.as_path(),
        );
        launcher_request
            .validate()
            .map_err(|reason| SandboxProviderError::PrepareFailed { reason })?;
        let payload = serde_json::to_vec(&launcher_request).map_err(|error| {
            SandboxProviderError::PrepareFailed {
                reason: format!("failed to encode Linux launcher request: {error}"),
            }
        })?;
        if payload.len() > protocol::MAX_LAUNCHER_PAYLOAD_BYTES {
            return prepare_failed("Linux launcher request exceeds the payload limit");
        }

        let command = SandboxExecutionCommand::new(
            self.config.helper_executable.clone(),
            vec!["--outer".to_string()],
            BTreeMap::new(),
            request.command().cwd().clone(),
            SandboxStdin::Bytes(payload),
        )?;
        Ok(PreparedSandboxCommand::from_command(command))
    }
}

pub fn run_launcher() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return launcher::run();
    #[cfg(not(target_os = "linux"))]
    Err("Linux sandbox launcher is only available on Linux".to_string())
}

#[cfg(target_os = "linux")]
fn require_file(path: &Path, label: &str) -> Result<(), SandboxProviderError> {
    if path.is_file() {
        Ok(())
    } else {
        probe_failed(format!("{label} is unavailable: {}", path.display()))
    }
}

#[cfg(target_os = "linux")]
fn probe_seccomp(helper: &Path) -> Result<(), SandboxProviderError> {
    let output = Command::new(helper)
        .arg("--probe-seccomp")
        .output()
        .map_err(|error| SandboxProviderError::ProbeFailed {
            reason: format!("failed to run seccomp preflight: {error}"),
        })?;
    if output.status.success() {
        Ok(())
    } else {
        probe_failed(format!(
            "seccomp preflight failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(target_os = "linux")]
fn probe_bubblewrap(bwrap: &Path) -> Result<(), SandboxProviderError> {
    let output = Command::new(bwrap)
        .args([
            "--new-session",
            "--die-with-parent",
            "--ro-bind",
            "/",
            "/",
            "--unshare-user",
            "--unshare-pid",
            "--proc",
            "/proc",
            "--",
            "/bin/true",
        ])
        .output()
        .map_err(|error| SandboxProviderError::ProbeFailed {
            reason: format!("failed to run bubblewrap preflight: {error}"),
        })?;
    if output.status.success() {
        Ok(())
    } else {
        probe_failed(format!(
            "bubblewrap user/PID namespace and /proc preflight failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn probe_failed<T>(reason: impl Into<String>) -> Result<T, SandboxProviderError> {
    Err(SandboxProviderError::ProbeFailed {
        reason: reason.into(),
    })
}

fn prepare_failed<T>(reason: impl Into<String>) -> Result<T, SandboxProviderError> {
    Err(SandboxProviderError::PrepareFailed {
        reason: reason.into(),
    })
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
