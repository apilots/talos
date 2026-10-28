//! Browser-owned dependency cache; no plugin install/enabled state or arbitrary installers.
use super::*;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, process::Stdio};
use tokio::process::Command;

const HOST_API: u32 = 1;
const INSPECT: &str = r#"import importlib.metadata as m,json,platform,sys
print(json.dumps({'platform':sys.platform+'-'+platform.machine(),'pythonAbi':sys.implementation.cache_tag,'packages':{d.metadata['Name'].lower().replace('_','-'):d.version for d in m.distributions()}}))"#;

#[derive(Default)]
pub(super) struct ManagedEnvironmentBinding {
    resources: Option<PathBuf>,
    active: Option<(String, PathBuf)>,
}

/// An already verified environment. It cannot activate itself or alter plugin state.
pub struct PreparedBrowserEnvironment {
    backend: Arc<dyn BrowserBackend>,
    identity: (String, PathBuf),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EnvironmentMetadata {
    lock_sha256: String,
    platform: String,
    python_abi: String,
    host_api: u32,
    python: PathBuf,
    browser_use_version: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PythonEnvironment {
    platform: String,
    python_abi: String,
    packages: BTreeMap<String, String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Preparation {
    ReuseOnly,
    InstallMissing,
}

impl BrowserRuntime {
    /// Only the Docker composition root supplies its product-owned resources.
    pub fn configure_managed_resources(&self, resources: &Path) -> TalosResult<()> {
        if self.state.lock().map_err(|_| poisoned())?.host != BrowserHostKind::Docker
            || !resources.is_absolute()
            || !resources.join("runtime.json").is_file()
        {
            return Err(unavailable("Managed Browser resources are unavailable."));
        }
        let mut binding = self.managed_environment.lock().map_err(|_| poisoned())?;
        if binding
            .resources
            .as_deref()
            .is_some_and(|current| current != resources)
        {
            return Err(conflict(
                "Managed Browser resources are already configured.",
            ));
        }
        binding.resources = Some(resources.to_path_buf());
        Ok(())
    }

    /// Downloads only the verified official Browser's exact hash lock, before commit gates.
    /// Native Desktop exits before touching Python or the dependency lock.
    pub async fn prepare_browser_environment(
        &self,
        lock: &Path,
    ) -> TalosResult<Option<PreparedBrowserEnvironment>> {
        self.environment_from_lock(lock, Preparation::InstallMissing)
            .await
    }

    /// Startup can reuse a verified local environment without waiting for the network.
    pub async fn reuse_browser_environment(
        &self,
        lock: &Path,
    ) -> TalosResult<Option<PreparedBrowserEnvironment>> {
        self.environment_from_lock(lock, Preparation::ReuseOnly)
            .await
    }

    async fn environment_from_lock(
        &self,
        lock: &Path,
        mode: Preparation,
    ) -> TalosResult<Option<PreparedBrowserEnvironment>> {
        if self.state.lock().map_err(|_| poisoned())?.host != BrowserHostKind::Docker {
            return Ok(None);
        }
        let resources = self
            .managed_environment
            .lock()
            .map_err(|_| poisoned())?
            .resources
            .clone()
            .ok_or_else(|| unavailable("Managed Browser resources are not configured."))?;
        let browser = self.preferences_path.parent().ok_or_else(poisoned)?;
        let home = browser.parent().ok_or_else(poisoned)?.to_path_buf();
        let bytes = tokio::fs::read(lock).await.map_err(|error| {
            TalosError::internal_with_source("The Browser dependency lock is unavailable.", error)
        })?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        let pins = lock_pins(&bytes)?;
        let version = pins
            .get("browser-use")
            .ok_or_else(|| unavailable("The Browser lock must pin browser-use."))?;
        let system = inspect(Path::new("/usr/bin/python3")).await?;
        let packaged = resources.join("environment.json");
        if let Some(metadata) = reusable(&packaged, &resources, &hash, &system, &pins).await? {
            return prepared(home, &resources, metadata).map(Some);
        }
        let key = browser
            .join("environments")
            .join(format!("{hash}-{}-{}", system.platform, system.python_abi));
        let Some(_lock) = preparation_lock(key.clone(), mode).await? else {
            return Ok(None);
        };
        let metadata_path = key.join("environment.json");
        if let Some(metadata) = reusable(&metadata_path, &key, &hash, &system, &pins).await? {
            return prepared(home, &resources, metadata).map(Some);
        }
        if mode == Preparation::ReuseOnly {
            return Ok(None);
        }
        // Venvs contain absolute paths: create each generation in its final location.
        // Preparing a replacement never modifies an active environment.
        let generation_directory = tempfile::Builder::new()
            .prefix("venv_")
            .tempdir_in(&key)
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Browser environment staging is unavailable.",
                    error,
                )
            })?;
        let generation = generation_directory.path();
        let mut create = Command::new("/usr/bin/python3");
        create.args(["-I", "-m", "venv"]).arg(generation);
        run(create, Duration::from_secs(60)).await?;
        let python = generation.join("bin/python");
        let mut install = Command::new(&python);
        install
            .args([
                "-I",
                "-m",
                "pip",
                "--isolated",
                "--disable-pip-version-check",
                "--no-input",
                "install",
                "--require-hashes",
                "--only-binary=:all:",
                "-r",
            ])
            .arg(lock);
        run(install, Duration::from_secs(240)).await?;
        let metadata = EnvironmentMetadata {
            lock_sha256: hash,
            platform: system.platform,
            python_abi: system.python_abi,
            host_api: HOST_API,
            python,
            browser_use_version: version.clone(),
        };
        if !matches_environment(&metadata, &pins).await? {
            return Err(unavailable(
                "The prepared Browser environment does not match its exact dependency lock.",
            ));
        }
        talos_fs::write_json_pretty(&metadata_path, &metadata)?;
        let _ = generation_directory.keep();
        prepared(home, &resources, metadata).map(Some)
    }

    /// Runs only after a durable Plugin commit under the original execution boundary.
    pub async fn activate_browser_environment(
        &self,
        prepared: PreparedBrowserEnvironment,
    ) -> TalosResult<()> {
        let changed = {
            let mut binding = self.managed_environment.lock().map_err(|_| poisoned())?;
            match &binding.active {
                Some(active) if *active == prepared.identity => return Ok(()),
                Some(_) => true,
                None => {
                    self.set_backend(prepared.backend)?;
                    binding.active = Some(prepared.identity);
                    false
                }
            }
        };
        if changed {
            self.restart_required
                .store(true, std::sync::atomic::Ordering::Release);
            self.plugin_disabled().await?;
        }
        Ok(())
    }
}

async fn preparation_lock(
    key: PathBuf,
    mode: Preparation,
) -> TalosResult<Option<talos_fs::FileLock>> {
    tokio::task::spawn_blocking(move || {
        use talos_fs::FileSystem;
        talos_fs::create_directory(&key, true)?;
        match mode {
            Preparation::ReuseOnly => talos_fs::std_fs().try_file_lock(&key.join("prepare.lock")),
            Preparation::InstallMissing => talos_fs::std_fs()
                .with_file_lock(&key.join("prepare.lock"))
                .map(Some),
        }
    })
    .await
    .map_err(|error| {
        TalosError::internal_with_source("Browser environment preparation could not start.", error)
    })?
}

fn prepared(
    home: PathBuf,
    resources: &Path,
    metadata: EnvironmentMetadata,
) -> TalosResult<PreparedBrowserEnvironment> {
    Ok(PreparedBrowserEnvironment {
        backend: crate::ManagedBrowser::prepared(
            home,
            resources,
            &metadata.python,
            &metadata.browser_use_version,
        )?,
        identity: (metadata.lock_sha256, metadata.python),
    })
}

fn lock_pins(bytes: &[u8]) -> TalosResult<BTreeMap<String, String>> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        TalosError::internal_with_source("The Browser lock is not valid text.", error)
    })?;
    let mut pins = BTreeMap::new();
    for line in text.lines().filter(|line| {
        !line.is_empty() && !line.starts_with(char::is_whitespace) && !line.starts_with('#')
    }) {
        let (name, version) = line
            .trim_end_matches([' ', '\\'])
            .split_once("==")
            .ok_or_else(|| unavailable("Browser dependencies must use exact package versions."))?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            || version.is_empty()
            || version.chars().any(char::is_whitespace)
            || pins
                .insert(
                    name.to_ascii_lowercase().replace('_', "-"),
                    version.to_owned(),
                )
                .is_some()
        {
            return Err(unavailable("The Browser dependency lock is invalid."));
        }
    }
    if pins.is_empty() || !text.contains("--hash=sha256:") {
        return Err(unavailable(
            "The Browser dependency lock must include package hashes.",
        ));
    }
    Ok(pins)
}

async fn inspect(python: &Path) -> TalosResult<PythonEnvironment> {
    let mut command = Command::new(python);
    command
        .args(["-I", "-c", INSPECT])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(15), command.output())
        .await
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::Timeout,
                "Browser environment validation timed out.",
                error,
            )
        })?
        .map_err(|error| {
            TalosError::internal_with_source(
                "The Browser Python environment could not be inspected.",
                error,
            )
        })?;
    if !output.status.success() || output.stdout.len() > 1024 * 1024 {
        return Err(unavailable(
            "The Browser Python environment is unavailable.",
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| {
        TalosError::internal_with_source("The Browser environment identity is invalid.", error)
    })
}

async fn reusable(
    path: &Path,
    root: &Path,
    hash: &str,
    system: &PythonEnvironment,
    pins: &BTreeMap<String, String>,
) -> TalosResult<Option<EnvironmentMetadata>> {
    if !path.is_file() {
        return Ok(None);
    }
    let metadata: EnvironmentMetadata = match talos_fs::read_json(path) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if metadata.host_api != HOST_API
        || metadata.lock_sha256 != hash
        || metadata.platform != system.platform
        || metadata.python_abi != system.python_abi
        || !metadata.python.is_absolute()
        || !metadata.python.starts_with(root)
        || !metadata.python.is_file()
        || pins.get("browser-use") != Some(&metadata.browser_use_version)
    {
        return Ok(None);
    }
    if matches_environment(&metadata, pins).await.unwrap_or(false) {
        Ok(Some(metadata))
    } else {
        Ok(None)
    }
}

async fn matches_environment(
    metadata: &EnvironmentMetadata,
    pins: &BTreeMap<String, String>,
) -> TalosResult<bool> {
    let actual = inspect(&metadata.python).await?;
    if actual.platform != metadata.platform
        || actual.python_abi != metadata.python_abi
        || !pins
            .iter()
            .all(|(name, version)| actual.packages.get(name) == Some(version))
    {
        return Ok(false);
    }
    let mut check = Command::new(&metadata.python);
    check.args(["-I", "-m", "pip", "check"]);
    Ok(run(check, Duration::from_secs(15)).await.is_ok())
}

async fn run(mut command: Command, budget: Duration) -> TalosResult<()> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(budget, command.status())
        .await
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::Timeout,
                "Browser dependency preparation timed out.",
                error,
            )
        })?
        .map_err(|error| {
            TalosError::internal_with_source(
                "Browser dependency preparation could not start.",
                error,
            )
        })?;
    if !status.success() {
        return Err(unavailable(
            "Browser dependency validation or installation failed.",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;
