//! Worker-owned executable search path for the local Bash and Grep tools.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::{env, fs};

use talos_config::ToolEnvironmentConfig;
use talos_tools_sandbox::AbsolutePathBuf;

mod ripgrep;
pub(crate) use ripgrep::RipgrepRecovery;

/// Immutable executable search path for one executing Worker. Construct at Worker
/// startup from trusted operator configuration, not from an invocation or client.
#[derive(Clone)]
pub struct WorkerToolEnvironment {
    configured: Vec<PathBuf>,
    implicit: Vec<PathBuf>,
    packaged_ripgrep: bool,
    ripgrep_recovery: Option<Arc<RipgrepRecovery>>,
}

impl WorkerToolEnvironment {
    /// Probe the Worker account once and merge its PATH with configured and inherited entries.
    /// Probe failures use only the configured and inherited entries; no global state is changed.
    pub async fn discover(configured: &[PathBuf], workspace_root: &Path) -> Self {
        let inherited = env::var_os("PATH");
        let result = probe::discover_path(inherited.as_deref(), workspace_root).await;
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
        let valid = |path: &PathBuf| ToolEnvironmentConfig::is_valid_path_entry(path);
        Self {
            configured: configured
                .iter()
                .filter(|path| valid(path))
                .cloned()
                .collect(),
            implicit: [discovered, inherited]
                .into_iter()
                .flatten()
                .flat_map(env::split_paths)
                .filter(valid)
                .collect(),
            packaged_ripgrep: false,
            ripgrep_recovery: None,
        }
    }

    /// Marks a product image that supplies ripgrep at build time. Set by the
    /// trusted Worker composition root, never by a tool invocation.
    pub fn with_packaged_ripgrep(mut self) -> Self {
        self.packaged_ripgrep = true;
        self
    }

    /// Enables the native, user-approved route under the Worker's private home.
    /// The managed directory is searched after configured, discovered and
    /// inherited paths, so an installed system executable keeps precedence.
    pub fn with_native_ripgrep(mut self, home: &Path, hostname: Option<&str>) -> Self {
        self.ripgrep_recovery = RipgrepRecovery::new(home, hostname).map(Arc::new);
        self
    }

    pub(crate) fn ripgrep_recovery(&self) -> Option<&RipgrepRecovery> {
        self.ripgrep_recovery.as_deref()
    }

    pub(crate) fn packaged_ripgrep(&self) -> bool {
        self.packaged_ripgrep
    }

    pub(crate) fn missing_ripgrep(packaged: bool) -> talos_error::TalosError {
        let message = if packaged {
            "The executing Worker image should already contain ripgrep (rg). Check its PATH (including agent.kernel.tools.environment.path_prepend), or rebuild or replace the image."
        } else {
            "Grep requires ripgrep (rg) on the executing Worker. Install ripgrep there, or add its executable directory to agent.kernel.tools.environment.path_prepend, then restart the Worker."
        };
        talos_error::TalosError::new(talos_error::ErrorCode::ResourceNotFound, message)
    }

    pub(crate) fn search_path(&self, workspace_roots: &[&Path]) -> ToolSearchPath {
        let workspace_roots: Vec<_> = workspace_roots
            .iter()
            .flat_map(|root| {
                [
                    AbsolutePathBuf::new(root.to_path_buf()).ok(),
                    root.canonicalize()
                        .ok()
                        .and_then(|path| AbsolutePathBuf::new(path).ok()),
                ]
            })
            .flatten()
            .collect();
        let implicit = self.implicit.iter().map(PathBuf::as_path).chain(
            self.ripgrep_recovery()
                .filter(|recovery| recovery.directory_is_current())
                .map(RipgrepRecovery::root),
        );
        let mut seen = HashSet::new();
        let entries = self
            .configured
            .iter()
            .map(PathBuf::as_path)
            .chain(implicit.filter(|path| outside_workspace(path, &workspace_roots)));
        let path = env::join_paths(entries.filter(|path| seen.insert(path.as_os_str())))
            .expect("validated native PATH entries are joinable")
            .into_string()
            .expect("validated PATH entries are UTF-8");
        // Some shells search the working directory when PATH is empty.
        #[cfg(unix)]
        if path.is_empty() {
            return ToolSearchPath("/dev/null".into());
        }
        ToolSearchPath(path)
    }
}

fn outside_workspace(path: &Path, roots: &[AbsolutePathBuf]) -> bool {
    roots.is_empty()
        || path.canonicalize().is_ok_and(|canonical| {
            let normalized = AbsolutePathBuf::new(path.to_owned());
            roots.iter().all(|root| {
                !canonical.starts_with(root.as_path())
                    && normalized
                        .as_ref()
                        .is_ok_and(|path| !path.as_path().starts_with(root.as_path()))
            })
        })
}

pub(crate) struct ToolSearchPath(String);

impl ToolSearchPath {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
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
        env::split_paths(OsStr::new(self.as_str()))
            .filter(|directory| directory.is_absolute())
            .map(|directory| directory.join(path))
            .find(|candidate| is_executable(candidate))
    }
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
    use super::{OsStr, Path};
    use std::ffi::OsString;

    pub(super) async fn discover_path(
        _: Option<&OsStr>,
        _: &Path,
    ) -> Result<OsString, &'static str> {
        Err("unsupported_os")
    }
}

#[cfg(test)]
#[path = "worker_environment_tests.rs"]
mod tests;
