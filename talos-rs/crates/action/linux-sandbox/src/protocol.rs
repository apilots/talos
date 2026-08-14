use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use talos_sandbox_spi::{NetworkIsolationSpec, SandboxExecutionRequest, SandboxStdin};

pub(crate) const PROTOCOL_VERSION: u16 = 1;
pub(crate) const MAX_LAUNCHER_PAYLOAD_BYTES: usize = 1024 * 1024;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct LauncherRequest {
    pub(crate) protocol_version: u16,
    pub(crate) bubblewrap_executable: PathBuf,
    pub(crate) helper_executable: PathBuf,
    pub(crate) command: LauncherCommand,
    pub(crate) file_system: LauncherFileSystem,
    pub(crate) network: NetworkIsolationSpec,
}

impl LauncherRequest {
    pub(crate) fn from_spi(
        protocol_version: u16,
        request: &SandboxExecutionRequest,
        bubblewrap_executable: &Path,
        helper_executable: &Path,
    ) -> Self {
        Self {
            protocol_version,
            bubblewrap_executable: bubblewrap_executable.to_path_buf(),
            helper_executable: helper_executable.to_path_buf(),
            command: LauncherCommand {
                program: request.command().program().as_path().to_path_buf(),
                args: request.command().args().to_vec(),
                env: request.command().env().clone(),
                cwd: request.command().cwd().as_path().to_path_buf(),
                stdin: request.command().stdin().clone(),
            },
            file_system: LauncherFileSystem {
                readable_roots: request
                    .file_system()
                    .readable_roots()
                    .iter()
                    .map(|path| path.as_path().to_path_buf())
                    .collect(),
                writable_roots: request
                    .file_system()
                    .writable_roots()
                    .iter()
                    .map(|path| path.as_path().to_path_buf())
                    .collect(),
                denied_roots: request
                    .file_system()
                    .denied_roots()
                    .iter()
                    .map(|path| path.as_path().to_path_buf())
                    .collect(),
            },
            network: request.network(),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(format!(
                "unsupported launcher protocol version: {}",
                self.protocol_version
            ));
        }
        require_file(&self.bubblewrap_executable, "bubblewrap executable")?;
        require_file(&self.helper_executable, "launcher helper")?;
        require_file(&self.command.program, "target program")?;
        require_directory(&self.command.cwd, "target cwd")?;
        for root in &self.file_system.readable_roots {
            require_absolute(root, "readable root")?;
        }
        for root in &self.file_system.writable_roots {
            require_directory(root, "writable root")?;
        }
        for root in &self.file_system.denied_roots {
            require_directory(root, "denied root")?;
        }
        Ok(())
    }
}

fn require_absolute(path: &Path, label: &str) -> Result<(), String> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(format!("{label} must be absolute: {}", path.display()))
    }
}

fn require_file(path: &Path, label: &str) -> Result<(), String> {
    require_absolute(path, label)?;
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("{label} is not a file: {}", path.display()))
    }
}

fn require_directory(path: &Path, label: &str) -> Result<(), String> {
    require_absolute(path, label)?;
    if path.is_dir() {
        Ok(())
    } else {
        Err(format!("{label} is not a directory: {}", path.display()))
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct LauncherCommand {
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) cwd: PathBuf,
    pub(crate) stdin: SandboxStdin,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct LauncherFileSystem {
    pub(crate) readable_roots: Vec<PathBuf>,
    pub(crate) writable_roots: Vec<PathBuf>,
    pub(crate) denied_roots: Vec<PathBuf>,
}
