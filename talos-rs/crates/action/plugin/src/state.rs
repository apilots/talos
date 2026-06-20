use fs4::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use talos_config::resolve_talos_home;
use talos_error::{TalosError, TalosResult};
use talos_skills::SkillState;
use talos_types::{
    MarketplaceSource, MarketplaceSourceId, Plugin, PluginInstall, PluginInstallId, PluginWarning,
};

use crate::installer::manifest_for_dir;

#[derive(Debug, Clone)]
pub struct PluginStateStore {
    root: PathBuf,
    #[cfg(test)]
    fail_next_save_installs: Arc<AtomicBool>,
    #[cfg(test)]
    fail_next_save_skill_state: Arc<AtomicBool>,
}

impl PluginStateStore {
    pub fn from_runtime_home() -> Self {
        Self {
            root: resolve_talos_home().join("plugins"),
            #[cfg(test)]
            fail_next_save_installs: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            fail_next_save_skill_state: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            #[cfg(test)]
            fail_next_save_installs: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            fail_next_save_skill_state: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(test)]
    pub(crate) fn fail_next_save_installs(&self) {
        self.fail_next_save_installs.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn fail_next_save_skill_state(&self) {
        self.fail_next_save_skill_state
            .store(true, Ordering::SeqCst);
    }

    pub fn sources_path(&self) -> PathBuf {
        self.root.join("marketplace-sources.json")
    }

    pub fn installs_path(&self) -> PathBuf {
        self.root.join("plugin-installs.json")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.root.join("plugin-state.lock")
    }

    pub fn installs_dir(&self) -> PathBuf {
        self.root.join("installs")
    }

    pub fn install_dir(&self, install_id: &PluginInstallId) -> PathBuf {
        self.installs_dir().join(&install_id.0)
    }

    pub fn install_temp_dir(&self, install_id: &PluginInstallId) -> PathBuf {
        self.installs_dir().join(format!(".tmp-{}", install_id.0))
    }

    pub fn install_backup_dir(&self, install_id: &PluginInstallId) -> PathBuf {
        self.installs_dir()
            .join(format!(".backup-{}", install_id.0))
    }

    pub fn install_trash_dir(&self, install_id: &PluginInstallId) -> PathBuf {
        self.installs_dir().join(format!(".trash-{}", install_id.0))
    }

    pub fn cleanup_install_temps(&self) -> TalosResult<()> {
        let installs_dir = self.installs_dir();
        if !installs_dir.exists() {
            return Ok(());
        }
        let installs_by_id = self
            .load_installs()?
            .installs
            .into_iter()
            .map(|install| (install.id.0.clone(), install))
            .collect::<HashMap<_, _>>();
        for entry in fs::read_dir(&installs_dir).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to read plugin installs dir `{}`: {err}",
                installs_dir.display()
            ))
        })? {
            let entry = entry.map_err(|err| {
                TalosError::StorageError(format!(
                    "failed to read plugin installs dir `{}` entry: {err}",
                    installs_dir.display()
                ))
            })?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() && name.starts_with(".trash-") {
                let install_id = name.trim_start_matches(".trash-");
                let install_dir = installs_dir.join(install_id);
                if installs_by_id.contains_key(install_id) {
                    restore_transaction_dir(&path, &install_dir, "plugin uninstall trash")?;
                } else {
                    remove_transaction_dir(&path, "plugin uninstall trash")?;
                }
                continue;
            }
            if path.is_dir() && name.starts_with(".backup-") {
                let install_id = name.trim_start_matches(".backup-");
                let install_dir = installs_dir.join(install_id);
                if let Some(install) = installs_by_id.get(install_id) {
                    if install_dir_matches_record(&install_dir, install)? {
                        remove_transaction_dir(&path, "plugin install backup")?;
                    } else {
                        restore_transaction_dir(&path, &install_dir, "plugin install backup")?;
                    }
                } else {
                    remove_transaction_dir(&path, "plugin install backup")?;
                }
                continue;
            }
            if path.is_dir() && name.starts_with(".tmp-") {
                remove_transaction_dir(&path, "plugin install temp")?;
            }
        }
        Ok(())
    }

    pub fn with_exclusive_lock<T>(
        &self,
        operation: &str,
        action: impl FnOnce() -> TalosResult<T>,
    ) -> TalosResult<T> {
        fs::create_dir_all(&self.root).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to create plugin state root `{}`: {err}",
                self.root.display()
            ))
        })?;
        let path = self.lock_path();
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|err| {
                TalosError::StorageError(format!(
                    "failed to open plugin state lock `{}`: {err}",
                    path.display()
                ))
            })?;
        FileExt::lock(&file).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to lock plugin state for {operation} `{}`: {err}",
                path.display()
            ))
        })?;
        let result = action();
        let unlock_result = FileExt::unlock(&file).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to unlock plugin state for {operation} `{}`: {err}",
                path.display()
            ))
        });
        match (result, unlock_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(err), _) => Err(err),
            (Ok(_), Err(err)) => Err(err),
        }
    }

    pub fn skills_state_path(&self) -> PathBuf {
        self.root.join("skills").join("skills-state.json")
    }

    pub fn load_skill_state(&self) -> TalosResult<SkillState> {
        read_json_or_default(&self.skills_state_path())
    }

    pub fn save_skill_state(&self, state: &SkillState) -> TalosResult<()> {
        #[cfg(test)]
        if self
            .fail_next_save_skill_state
            .swap(false, Ordering::SeqCst)
        {
            return Err(TalosError::StorageError(
                "injected plugin skill state save failure".to_string(),
            ));
        }
        write_json_atomic(&self.skills_state_path(), state)
    }

    pub fn marketplace_cache_dir(&self, source_id: &MarketplaceSourceId) -> PathBuf {
        self.root
            .join("cache")
            .join("marketplaces")
            .join(&source_id.0)
    }

    pub fn source_scan_cache_path(&self, source_id: &MarketplaceSourceId) -> PathBuf {
        self.marketplace_cache_dir(source_id)
            .join("scan-cache.json")
    }

    pub fn load_sources(&self) -> TalosResult<MarketplaceSourcesFile> {
        read_json_or_default(&self.sources_path())
    }

    pub fn save_sources(&self, sources: &MarketplaceSourcesFile) -> TalosResult<()> {
        write_json_atomic(&self.sources_path(), sources)
    }

    pub fn load_installs(&self) -> TalosResult<PluginInstallsFile> {
        read_json_or_default(&self.installs_path())
    }

    pub fn save_installs(&self, installs: &PluginInstallsFile) -> TalosResult<()> {
        #[cfg(test)]
        if self.fail_next_save_installs.swap(false, Ordering::SeqCst) {
            return Err(TalosError::StorageError(
                "injected plugin installs save failure".to_string(),
            ));
        }
        write_json_atomic(&self.installs_path(), installs)
    }

    pub fn load_source_scan_cache(
        &self,
        source_id: &MarketplaceSourceId,
    ) -> TalosResult<SourceScanCacheFile> {
        read_json_or_default(&self.source_scan_cache_path(source_id))
    }

    pub fn save_source_scan_cache(
        &self,
        source_id: &MarketplaceSourceId,
        cache: &SourceScanCacheFile,
    ) -> TalosResult<()> {
        write_json_atomic(&self.source_scan_cache_path(source_id), cache)
    }

    pub fn remove_marketplace_cache(&self, source_id: &MarketplaceSourceId) -> TalosResult<()> {
        let path = self.marketplace_cache_dir(source_id);
        if !path.exists() {
            return Ok(());
        }
        fs::remove_dir_all(&path).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to remove plugin source cache `{}`: {err}",
                path.display()
            ))
        })
    }
}

fn install_dir_matches_record(install_dir: &Path, install: &PluginInstall) -> TalosResult<bool> {
    if !install_dir.exists() {
        return Ok(false);
    }
    let skill_dir = install_dir.join("skills").join(&install.name);
    Ok(manifest_for_dir(&skill_dir)? == install.file_manifest)
}

fn restore_transaction_dir(source: &Path, target: &Path, label: &str) -> TalosResult<()> {
    if target.exists() {
        fs::remove_dir_all(target).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to remove stale plugin install dir `{}` before restoring {label}: {err}",
                target.display()
            ))
        })?;
    }
    fs::rename(source, target).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to restore {label} `{}` to `{}`: {err}",
            source.display(),
            target.display()
        ))
    })
}

fn remove_transaction_dir(path: &Path, label: &str) -> TalosResult<()> {
    fs::remove_dir_all(path).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to remove {label} dir `{}`: {err}",
            path.display()
        ))
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketplaceSourcesFile {
    pub version: u32,
    pub sources: Vec<MarketplaceSource>,
}

impl Default for MarketplaceSourcesFile {
    fn default() -> Self {
        Self {
            version: 1,
            sources: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceScanCacheFile {
    pub version: u32,
    pub source_id: MarketplaceSourceId,
    pub scanned_at_ms: i64,
    pub source_commit: Option<String>,
    pub plugins: Vec<Plugin>,
    #[serde(default)]
    pub warnings: Vec<PluginWarning>,
}

impl Default for SourceScanCacheFile {
    fn default() -> Self {
        Self {
            version: 1,
            source_id: MarketplaceSourceId(String::new()),
            scanned_at_ms: 0,
            source_commit: None,
            plugins: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInstallsFile {
    pub version: u32,
    pub installs: Vec<PluginInstall>,
}

impl Default for PluginInstallsFile {
    fn default() -> Self {
        Self {
            version: 1,
            installs: Vec::new(),
        }
    }
}

fn read_json_or_default<T>(path: &Path) -> TalosResult<T>
where
    T: Default + for<'de> Deserialize<'de>,
{
    if !path.exists() {
        return Ok(T::default());
    }
    let contents = fs::read_to_string(path).map_err(|err| {
        TalosError::StorageError(format!("failed to read `{}`: {err}", path.display()))
    })?;
    serde_json::from_str(&contents).map_err(|err| {
        TalosError::SerializationError(format!("failed to parse `{}`: {err}", path.display()))
    })
}

fn write_json_atomic<T>(path: &Path, value: &T) -> TalosResult<()>
where
    T: Serialize,
{
    let parent = path.parent();
    if let Some(parent) = parent {
        fs::create_dir_all(parent).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to create plugin state directory `{}`: {err}",
                parent.display()
            ))
        })?;
    }
    let contents = serde_json::to_string_pretty(value).map_err(|err| {
        TalosError::SerializationError(format!("failed to serialize plugin state: {err}"))
    })?;
    let temp = path.with_extension("tmp");
    {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temp)
            .map_err(|err| {
                TalosError::StorageError(format!("failed to open `{}`: {err}", temp.display()))
            })?;
        file.write_all(contents.as_bytes()).map_err(|err| {
            TalosError::StorageError(format!("failed to write `{}`: {err}", temp.display()))
        })?;
        file.sync_all().map_err(|err| {
            TalosError::StorageError(format!("failed to sync `{}`: {err}", temp.display()))
        })?;
    }
    fs::rename(&temp, path).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to replace `{}` with `{}`: {err}",
            path.display(),
            temp.display()
        ))
    })?;
    if let Some(parent) = parent {
        File::open(parent)
            .and_then(|file| file.sync_all())
            .map_err(|err| {
                TalosError::StorageError(format!(
                    "failed to sync plugin state directory `{}`: {err}",
                    parent.display()
                ))
            })?;
    }
    Ok(())
}
