use super::PluginManager;
use crate::documents::{
    document_media_type, document_priority, document_role, document_title, readme_document_names,
};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use talos_error::{TalosError, TalosResult};
use talos_types::{
    Plugin, PluginDocument, PluginReadRequest, PluginReadResponse, PluginSourceListRequest,
    PluginStatus,
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
        let documents = self.plugin_documents(&plugin)?;
        Ok(PluginReadResponse { plugin, documents })
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
            return Ok(self.plugin_for_install(install, install.enabled));
        }
        Err(TalosError::ResourceNotFound(format!(
            "plugin `{}` not found",
            plugin_id.0
        )))
    }

    fn plugin_documents(&self, plugin: &Plugin) -> TalosResult<Vec<PluginDocument>> {
        let (root, primary_path) = self.plugin_document_root_and_primary_path(plugin)?;
        let mut paths = document_paths(plugin, &root, &primary_path)?;
        paths.sort_by_key(|path| document_priority(plugin.kind, path));
        paths.dedup();

        let mut documents = Vec::new();
        for path in paths {
            let contents = read_to_string_limited(&path, self.max_skill_md_bytes)?;
            documents.push(PluginDocument {
                role: document_role(plugin.kind, &path),
                path: relative_document_path(&root, &path)?,
                title: document_title(&path),
                media_type: document_media_type(&path),
                contents,
            });
        }
        Ok(documents)
    }

    fn plugin_document_root_and_primary_path(
        &self,
        plugin: &Plugin,
    ) -> TalosResult<(PathBuf, PathBuf)> {
        if let Some(installed_path) = installed_path(plugin) {
            let root = installed_content_root(plugin, installed_path)?;
            let path = root.join(installed_document_name(plugin)?);
            let path = ensure_contained_regular_file(&root, &path)?;
            let root = fs::canonicalize(&root).map_err(|err| {
                TalosError::StorageError(format!(
                    "failed to canonicalize plugin document root `{}`: {err}",
                    root.display()
                ))
            })?;
            return Ok((root, path));
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
        let path = root.join(source_document_path(plugin)?);
        let path = ensure_contained_regular_file(&root, &path)?;
        let document_root = path.parent().ok_or_else(|| {
            TalosError::InternalError(format!(
                "plugin document `{}` has no parent directory",
                path.display()
            ))
        })?;
        Ok((document_root.to_path_buf(), path))
    }
}

fn document_paths(plugin: &Plugin, root: &Path, primary_path: &Path) -> TalosResult<Vec<PathBuf>> {
    let mut paths = Vec::new();
    paths.push(primary_path.to_path_buf());
    for readme_name in readme_document_names() {
        let readme_path = root.join(readme_name);
        if let Some(path) = optional_contained_regular_file(root, &readme_path)? {
            paths.push(path);
        }
    }
    if plugin.kind == talos_types::PluginKind::McpServer {
        let manifest_path = root.join("talos-mcp.json");
        let path = ensure_contained_regular_file(root, &manifest_path)?;
        paths.push(path);
    }
    Ok(paths)
}

fn relative_document_path(root: &Path, path: &Path) -> TalosResult<String> {
    let relative = path.strip_prefix(root).map_err(|err| {
        TalosError::InternalError(format!(
            "failed to build plugin document path for `{}`: {err}",
            path.display()
        ))
    })?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn installed_content_root(plugin: &Plugin, installed_path: &Path) -> TalosResult<PathBuf> {
    match plugin.kind {
        talos_types::PluginKind::Skill | talos_types::PluginKind::McpServer => {
            Ok(installed_path.to_path_buf())
        }
        other => Err(TalosError::ValidationError(format!(
            "plugin kind `{other:?}` has no readable document"
        ))),
    }
}

fn installed_document_name(plugin: &Plugin) -> TalosResult<&'static str> {
    match plugin.kind {
        talos_types::PluginKind::Skill => Ok("SKILL.md"),
        talos_types::PluginKind::McpServer => Ok("talos-mcp.json"),
        other => Err(TalosError::ValidationError(format!(
            "plugin kind `{other:?}` has no readable document"
        ))),
    }
}

fn source_document_path(plugin: &Plugin) -> TalosResult<&str> {
    match &plugin.definition {
        talos_types::PluginDefinition::Skill(skill) => Ok(skill.skill_md_path.as_str()),
        talos_types::PluginDefinition::McpServer(mcp) => Ok(mcp.manifest_path.as_str()),
        other => Err(TalosError::ValidationError(format!(
            "plugin definition `{other:?}` has no readable document"
        ))),
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

fn optional_contained_regular_file(root: &Path, path: &Path) -> TalosResult<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    ensure_contained_regular_file(root, path).map(Some)
}

fn read_to_string_limited(path: &Path, max_bytes: usize) -> TalosResult<String> {
    let metadata = fs::metadata(path).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to inspect plugin document `{}`: {err}",
            path.display()
        ))
    })?;
    if metadata.len() > max_bytes as u64 {
        return Err(TalosError::ValidationError(format!(
            "plugin document `{}` exceeds max size of {} bytes",
            path.display(),
            max_bytes
        )));
    }
    let mut file = File::open(path).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to read plugin document `{}`: {err}",
            path.display()
        ))
    })?;
    let mut contents = String::new();
    file.by_ref()
        .take(max_bytes.saturating_add(1) as u64)
        .read_to_string(&mut contents)
        .map_err(|err| {
            TalosError::StorageError(format!(
                "failed to read plugin document `{}`: {err}",
                path.display()
            ))
        })?;
    if contents.len() > max_bytes {
        return Err(TalosError::ValidationError(format!(
            "plugin document `{}` exceeds max size of {} bytes",
            path.display(),
            max_bytes
        )));
    }
    Ok(contents)
}
