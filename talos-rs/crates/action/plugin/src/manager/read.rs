use super::PluginManager;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use talos_error::{TalosError, TalosResult};
use talos_types::{
    Plugin, PluginReadRequest, PluginReadResponse, PluginSourceListRequest, PluginStatus,
};

impl PluginManager {
    pub fn read(&self, request: PluginReadRequest) -> TalosResult<PluginReadResponse> {
        self.state
            .with_exclusive_lock("plugin.read", || self.read_locked(request))
    }

    fn read_locked(&self, request: PluginReadRequest) -> TalosResult<PluginReadResponse> {
        self.state.cleanup_install_temps()?;
        let mut plugin = self.plugin_for_read(&request.plugin_id)?;
        self.merge_installed_statuses(std::slice::from_mut(&mut plugin))?;
        let path = self.plugin_skill_md_path(&plugin)?;
        let contents = read_to_string_limited(&path, self.max_skill_md_bytes)?;
        Ok(PluginReadResponse { plugin, contents })
    }

    fn plugin_for_read(&self, plugin_id: &talos_types::PluginId) -> TalosResult<Plugin> {
        if let Some(plugin) = self.find_cached_plugin(plugin_id) {
            return Ok(plugin);
        }
        let installs = self.state.load_installs()?.installs;
        if let Some(install) = installs
            .iter()
            .find(|install| install.plugin_id == *plugin_id)
        {
            let enabled = self.install_enabled(install)?;
            return Ok(self.plugin_for_install(install, enabled));
        }
        Err(TalosError::ResourceNotFound(format!(
            "plugin `{}` not found",
            plugin_id.0
        )))
    }

    fn plugin_skill_md_path(&self, plugin: &Plugin) -> TalosResult<PathBuf> {
        let talos_types::PluginDefinition::Skill(skill) = &plugin.definition else {
            return Err(TalosError::ValidationError(format!(
                "plugin `{}` is not a skill plugin",
                plugin.id.0
            )));
        };

        if let Some(installed_path) = installed_path(plugin) {
            let root = installed_path.join("skills").join(&plugin.name);
            let path = root.join("SKILL.md");
            return ensure_contained_regular_file(&root, &path);
        }

        let source_id = plugin.source_id.as_ref().ok_or_else(|| {
            TalosError::ValidationError(format!("plugin `{}` has no source id", plugin.id.0))
        })?;
        let source = self
            .list_sources(PluginSourceListRequest {})?
            .sources
            .into_iter()
            .find(|source| &source.id == source_id)
            .ok_or_else(|| {
                TalosError::ResourceNotFound(format!("plugin source `{}` not found", source_id.0))
            })?;
        let root = match &source.location {
            talos_types::MarketplaceSourceLocation::LocalFolder { path } => path.clone(),
            talos_types::MarketplaceSourceLocation::Git { .. } => {
                self.state.marketplace_cache_dir(&source.id).join("repo")
            }
        };
        let path = root.join(&skill.skill_md_path);
        ensure_contained_regular_file(&root, &path)
    }
}

fn installed_path(plugin: &Plugin) -> Option<&Path> {
    match &plugin.status {
        PluginStatus::Installed { installed_path, .. }
        | PluginStatus::UpdateAvailable { installed_path, .. }
        | PluginStatus::SourceMissing { installed_path, .. } => Some(installed_path.as_path()),
        _ => None,
    }
}

fn ensure_contained_regular_file(root: &Path, path: &Path) -> TalosResult<PathBuf> {
    let metadata = fs::symlink_metadata(path).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to inspect plugin document `{}`: {err}",
            path.display()
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(TalosError::ValidationError(format!(
            "plugin document `{}` is not a regular file",
            path.display()
        )));
    }
    let root = fs::canonicalize(root).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to canonicalize plugin root `{}`: {err}",
            root.display()
        ))
    })?;
    let path = fs::canonicalize(path).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to canonicalize plugin document `{}`: {err}",
            path.display()
        ))
    })?;
    if path.starts_with(&root) {
        return Ok(path);
    }
    Err(TalosError::ValidationError(format!(
        "plugin document `{}` escaped root `{}`",
        path.display(),
        root.display()
    )))
}

fn read_to_string_limited(path: &Path, max_bytes: usize) -> TalosResult<String> {
    let metadata = fs::metadata(path).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to inspect plugin skill document `{}`: {err}",
            path.display()
        ))
    })?;
    if metadata.len() > max_bytes as u64 {
        return Err(TalosError::ValidationError(format!(
            "plugin skill document `{}` exceeds max size of {} bytes",
            path.display(),
            max_bytes
        )));
    }
    let mut file = File::open(path).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to read plugin skill document `{}`: {err}",
            path.display()
        ))
    })?;
    let mut contents = String::new();
    file.by_ref()
        .take(max_bytes.saturating_add(1) as u64)
        .read_to_string(&mut contents)
        .map_err(|err| {
            TalosError::StorageError(format!(
                "failed to read plugin skill document `{}`: {err}",
                path.display()
            ))
        })?;
    if contents.len() > max_bytes {
        return Err(TalosError::ValidationError(format!(
            "plugin skill document `{}` exceeds max size of {} bytes",
            path.display(),
            max_bytes
        )));
    }
    Ok(contents)
}
