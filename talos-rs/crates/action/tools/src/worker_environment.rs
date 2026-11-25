//! Worker-owned executable search path for the local Bash and Grep tools.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use std::{env, fs};

/// Immutable executable search path for one executing Worker. Construct at Worker
/// startup from trusted operator configuration, not from an invocation or client.
#[derive(Clone)]
pub struct WorkerToolEnvironment {
    path: String,
    packaged_ripgrep: bool,
}

impl WorkerToolEnvironment {
    /// Probe the Worker account once and merge its PATH with configured and inherited entries.
    /// Probe failures use only the configured and inherited entries; no global state is changed.
    pub async fn discover(configured: &[PathBuf]) -> Self {
        let inherited = env::var_os("PATH");
        let result = probe::discover_path(inherited.as_deref()).await;
        match &result {
            Ok(_) => tracing::info!(os = env::consts::OS, "Worker tool PATH discovered"),
            Err(reason) => tracing::info!(
                os = env::consts::OS,
                ?reason,
                "Worker tool PATH discovery unavailable; using configured and inherited entries"
            ),
        }
        Self::assemble(configured, result.ok().as_deref(), inherited.as_deref())
    }

    pub(crate) fn inherited() -> Self {
        Self::assemble(&[], None, env::var_os("PATH").as_deref())
    }

    pub(crate) fn assemble(
        configured: &[PathBuf],
        discovered: Option<&OsStr>,
        inherited: Option<&OsStr>,
    ) -> Self {
        let mut seen = HashSet::<OsString>::new();
        let entries = configured.iter().cloned().chain(
            [discovered, inherited]
                .into_iter()
                .flatten()
                .flat_map(env::split_paths),
        );
        // Sandbox process environments are UTF-8. Filter before resolving so the
        // resolver and the sandbox child can never see different directories.
        let path = env::join_paths(entries.filter_map(|entry| {
            let entry = entry.into_os_string();
            (valid_entry(&entry) && seen.insert(entry.clone())).then_some(entry)
        }))
        .expect("validated native PATH entries are joinable")
        .into_string()
        .expect("validated PATH entries are UTF-8");
        Self {
            path,
            packaged_ripgrep: false,
        }
    }

    /// Marks a product image that supplies ripgrep at build time. Set by the
    /// trusted Worker composition root, never by a tool invocation.
    pub fn with_packaged_ripgrep(mut self) -> Self {
        self.packaged_ripgrep = true;
        self
    }

    pub(crate) fn missing_ripgrep(&self) -> talos_error::TalosError {
        let message = if self.packaged_ripgrep {
            "The executing Worker image should already contain ripgrep (rg). Check its PATH (including agent.kernel.tools.environment.path_prepend), or rebuild or replace the image."
        } else {
            "Grep requires ripgrep (rg) on the executing Worker. Install ripgrep there, or add its executable directory to agent.kernel.tools.environment.path_prepend, then restart the Worker."
        };
        talos_error::TalosError::new(talos_error::ErrorCode::ResourceNotFound, message)
    }

    pub(crate) fn path(&self) -> &str {
        // Some POSIX shells treat an empty PATH like a current-directory entry.
        // /dev/null is an absolute non-directory, so neither shell fallback nor
        // workspace lookup is possible when no usable entries exist.
        #[cfg(unix)]
        if self.path.is_empty() {
            return "/dev/null";
        }
        &self.path
    }

    pub(crate) fn resolve(&self, program: &str) -> Option<PathBuf> {
        let path = Path::new(program);
        if path.is_absolute() {
            return is_executable(path).then(|| path.to_path_buf());
        }
        if !matches!(path.components().next(), Some(Component::Normal(name)) if name == path.as_os_str())
        {
            return None;
        }
        env::split_paths(OsStr::new(&self.path))
            .filter(|directory| directory.is_absolute())
            .map(|directory| directory.join(path))
            .find(|candidate| is_executable(candidate))
    }
}

fn valid_entry(entry: &OsStr) -> bool {
    Path::new(entry).is_absolute()
        && entry.to_str().is_some_and(|entry| !entry.contains('\0'))
        && env::join_paths([entry]).is_ok()
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        // SAFETY: path is a live NUL-terminated string. Test execute access using
        // the Worker's effective UID/GID, including ACLs, not just file mode bits.
        metadata.is_file()
            && unsafe {
                libc::faccessat(libc::AT_FDCWD, path.as_ptr(), libc::X_OK, libc::AT_EACCESS)
            } == 0
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    metadata.is_file()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod probe;

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod probe {
    use super::{OsStr, OsString};

    pub(super) async fn discover_path(_: Option<&OsStr>) -> Result<OsString, &'static str> {
        Err("unsupported_os")
    }
}

#[cfg(test)]
#[path = "worker_environment_tests.rs"]
mod tests;
