use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use talos_config::resolve_talos_home;
use talos_error::{TalosError, TalosResult};
use talos_fs::FileSystem;
use talos_skills::SkillState;
use talos_types::{
    MarketplaceSource, MarketplaceSourceId, Plugin, PluginInstall, PluginInstallId, PluginWarning,
};

#[derive(Debug, Clone)]
pub struct PluginStateStore {
    root: PathBuf,
    #[cfg(test)]
    fail_next_save_installs: Arc<AtomicBool>,
}

impl PluginStateStore {
    pub fn from_runtime_home() -> Self {
        Self {
            root: resolve_talos_home().join("plugins"),
            #[cfg(test)]
            fail_next_save_installs: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            #[cfg(test)]
            fail_next_save_installs: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(test)]
    pub(crate) fn fail_next_save_installs(&self) {
        self.fail_next_save_installs.store(true, Ordering::SeqCst);
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

    pub fn skill_installs_dir(&self) -> PathBuf {
        self.root.join("skills")
    }

    pub fn mcp_installs_dir(&self) -> PathBuf {
        self.root.join("mcp")
    }

    pub fn transaction_temp_dir(&self, install_id: &PluginInstallId) -> PathBuf {
        self.root.join(".tmp").join(&install_id.0)
    }

    pub fn transaction_backup_dir(&self, install_id: &PluginInstallId) -> PathBuf {
        self.root.join(".backup").join(&install_id.0)
    }

    pub fn transaction_trash_dir(&self, install_id: &PluginInstallId) -> PathBuf {
        self.root.join(".trash").join(&install_id.0)
    }

    pub fn cleanup_install_temps(&self) -> TalosResult<()> {
        remove_transaction_root(&self.root.join(".tmp"), "plugin install temp")?;
        Ok(())
    }

    pub fn with_exclusive_lock<T>(
        &self,
        operation: &str,
        action: impl FnOnce() -> TalosResult<T>,
    ) -> TalosResult<T> {
        talos_fs::create_directory(&self.root, true).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to create plugin state root `{}`: {err}",
                self.root.display()
            ))
        })?;
        let path = self.lock_path();
        let lock = talos_fs::std_fs().with_file_lock(&path).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to lock plugin state for {operation} `{}`: {err}",
                path.display()
            ))
        })?;
        let result = action();
        let unlock_result = lock.unlock().map_err(|err| {
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
        talos_fs::read_json_or_default(&self.skills_state_path())
    }

    pub fn save_skill_state(&self, state: &SkillState) -> TalosResult<()> {
        talos_fs::write_json_pretty(&self.skills_state_path(), state)
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
        talos_fs::read_json_or_default(&self.sources_path())
    }

    pub fn save_sources(&self, sources: &MarketplaceSourcesFile) -> TalosResult<()> {
        talos_fs::write_json_pretty(&self.sources_path(), sources)
    }

    pub fn load_installs(&self) -> TalosResult<PluginInstallsFile> {
        talos_fs::read_json_or_default(&self.installs_path())
    }

    pub fn save_installs(&self, installs: &PluginInstallsFile) -> TalosResult<()> {
        #[cfg(test)]
        if self.fail_next_save_installs.swap(false, Ordering::SeqCst) {
            return Err(TalosError::StorageError(
                "injected plugin installs save failure".to_string(),
            ));
        }
        talos_fs::write_json_pretty(&self.installs_path(), installs)
    }

    pub fn load_source_scan_cache(
        &self,
        source_id: &MarketplaceSourceId,
    ) -> TalosResult<SourceScanCacheFile> {
        talos_fs::read_json_or_default(&self.source_scan_cache_path(source_id))
    }

    pub fn save_source_scan_cache(
        &self,
        source_id: &MarketplaceSourceId,
        cache: &SourceScanCacheFile,
    ) -> TalosResult<()> {
        talos_fs::write_json_pretty(&self.source_scan_cache_path(source_id), cache)
    }

    pub fn remove_marketplace_cache(&self, source_id: &MarketplaceSourceId) -> TalosResult<()> {
        let path = self.marketplace_cache_dir(source_id);
        if !path.exists() {
            return Ok(());
        }
        talos_fs::std_fs().remove_directory(&path).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to remove plugin source cache `{}`: {err}",
                path.display()
            ))
        })
    }
}

fn remove_transaction_root(path: &Path, label: &str) -> TalosResult<()> {
    if !path.exists() {
        return Ok(());
    }
    talos_fs::std_fs().remove_directory(path).map_err(|err| {
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
    pub schema_version: u32,
    pub installs: Vec<PluginInstall>,
}

impl Default for PluginInstallsFile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            installs: Vec::new(),
        }
    }
}
