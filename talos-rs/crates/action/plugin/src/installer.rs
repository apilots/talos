mod runtime;

pub(crate) use runtime::{
    installed_mcp_servers, installed_plugin, installed_skill_root, missing_plugin_from_install,
};

use crate::PluginStateStore;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use talos_error::{TalosError, TalosResult};
use talos_types::{
    FileEntry, FileManifest, MarketplaceSource, MarketplaceSourceLocation, Plugin,
    PluginDefinition, PluginId, PluginInstall, PluginInstallConflictPolicy, PluginInstallId,
    PluginInstallSource, PluginKind, PluginUpdateConflictPolicy,
};

pub(crate) struct InstallCandidate {
    pub plugin: Plugin,
    pub source: MarketplaceSource,
}

pub(crate) struct InstallActivation {
    pub install: PluginInstall,
    backup_dir: Option<PathBuf>,
}

pub(crate) struct UpdateActivation {
    pub plugin: Plugin,
    pub install: PluginInstall,
    backup_dir: Option<PathBuf>,
}

pub(crate) struct UninstallActivation {
    pub removed_path: bool,
    install_id: PluginInstallId,
    trash_dir: PathBuf,
}

pub(crate) fn install_plugin(
    state: &PluginStateStore,
    candidate: InstallCandidate,
    conflict_policy: PluginInstallConflictPolicy,
) -> TalosResult<InstallActivation> {
    ensure_installable_plugin(&candidate.plugin)?;
    let install_id = install_id_for(&candidate.plugin.id);
    let install_dir = state.install_dir(&install_id);
    let backup_dir = state.install_backup_dir(&install_id);
    state.cleanup_install_temps()?;
    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to clear plugin install backup dir `{}`: {err}",
                backup_dir.display()
            ))
        })?;
    }
    let replace_existing = if install_dir.exists() {
        match conflict_policy {
            PluginInstallConflictPolicy::Reject => {
                return Err(TalosError::Conflict(format!(
                    "plugin install `{}` already exists",
                    install_id.0
                )));
            }
            PluginInstallConflictPolicy::Replace => true,
        }
    } else {
        false
    };

    let temp_dir = state.install_temp_dir(&install_id);
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to clear plugin install temp dir `{}`: {err}",
                temp_dir.display()
            ))
        })?;
    }
    let file_manifest = materialize_install_dir(state, &candidate, &install_id, &temp_dir)?;
    if replace_existing {
        fs::rename(&install_dir, &backup_dir).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to stage existing plugin install `{}` for replace: {err}",
                install_dir.display()
            ))
        })?;
    }
    fs::rename(&temp_dir, &install_dir).map_err(|err| {
        let _ = if backup_dir.exists() {
            fs::rename(&backup_dir, &install_dir)
        } else {
            Ok(())
        };
        TalosError::StorageError(format!(
            "failed to activate plugin install `{}`: {err}",
            install_dir.display()
        ))
    })?;
    let now_ms = Utc::now().timestamp_millis();
    let exposed_skill_root = exposed_skill_root_for(&candidate.plugin, &install_dir);
    let exposed_mcp_servers = exposed_mcp_servers_for(&candidate.plugin, &install_dir)?;
    let install = PluginInstall {
        id: install_id.clone(),
        plugin_id: candidate.plugin.id.clone(),
        kind: candidate.plugin.kind,
        name: candidate.plugin.metadata.name.clone(),
        installed_path: install_dir,
        exposed_skill_root,
        exposed_mcp_servers,
        source: install_source(&candidate.source, &candidate.plugin),
        installed_at_ms: now_ms,
        updated_at_ms: now_ms,
        file_manifest,
    };
    Ok(InstallActivation {
        install,
        backup_dir: backup_dir.exists().then_some(backup_dir),
    })
}

pub(crate) fn commit_install_activation(activation: &InstallActivation) -> TalosResult<()> {
    if let Some(backup_dir) = &activation.backup_dir {
        if backup_dir.exists() {
            fs::remove_dir_all(backup_dir).map_err(|err| {
                TalosError::StorageError(format!(
                    "failed to remove plugin install backup `{}`: {err}",
                    backup_dir.display()
                ))
            })?;
        }
    }
    Ok(())
}

pub(crate) fn rollback_install_activation(
    state: &PluginStateStore,
    activation: &InstallActivation,
) -> TalosResult<()> {
    let install_dir = state.install_dir(&activation.install.id);
    if install_dir.exists() {
        fs::remove_dir_all(&install_dir).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to remove failed plugin install `{}`: {err}",
                install_dir.display()
            ))
        })?;
    }
    if let Some(backup_dir) = &activation.backup_dir {
        if backup_dir.exists() {
            fs::rename(backup_dir, &install_dir).map_err(|err| {
                TalosError::StorageError(format!(
                    "failed to restore plugin install backup `{}`: {err}",
                    backup_dir.display()
                ))
            })?;
        }
    }
    Ok(())
}

pub(crate) fn update_plugin(
    state: &PluginStateStore,
    install: &PluginInstall,
    candidate: InstallCandidate,
    conflict_policy: PluginUpdateConflictPolicy,
    enabled: bool,
) -> TalosResult<UpdateActivation> {
    ensure_installable_plugin(&candidate.plugin)?;
    if install.kind != candidate.plugin.kind || install.plugin_id != candidate.plugin.id {
        return Err(TalosError::Conflict(format!(
            "plugin install `{}` does not match update candidate `{}`",
            install.id.0, candidate.plugin.id.0
        )));
    }
    state.cleanup_install_temps()?;
    if has_local_changes(install)? {
        match conflict_policy {
            PluginUpdateConflictPolicy::RejectLocalChanges => {
                return Err(TalosError::Conflict(format!(
                    "plugin install `{}` has local changes",
                    install.id.0
                )));
            }
            PluginUpdateConflictPolicy::ReplaceLocalChanges => {}
        }
    }

    let install_dir = state.install_dir(&install.id);
    let temp_dir = state.install_temp_dir(&install.id);
    let backup_dir = state.install_backup_dir(&install.id);
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to clear plugin update temp dir `{}`: {err}",
                temp_dir.display()
            ))
        })?;
    }
    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to clear plugin update backup dir `{}`: {err}",
                backup_dir.display()
            ))
        })?;
    }
    let file_manifest = materialize_install_dir(state, &candidate, &install.id, &temp_dir)?;
    if install_dir.exists() {
        fs::rename(&install_dir, &backup_dir).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to stage plugin install `{}` for update: {err}",
                install_dir.display()
            ))
        })?;
    }
    fs::rename(&temp_dir, &install_dir).map_err(|err| {
        let _ = if backup_dir.exists() {
            fs::rename(&backup_dir, &install_dir)
        } else {
            Ok(())
        };
        TalosError::StorageError(format!(
            "failed to activate plugin update `{}`: {err}",
            install_dir.display()
        ))
    })?;

    let mut updated = install.clone();
    updated.name = candidate.plugin.metadata.name.clone();
    updated.installed_path = install_dir.clone();
    updated.exposed_skill_root = exposed_skill_root_for(&candidate.plugin, &install_dir);
    updated.exposed_mcp_servers = exposed_mcp_servers_for(&candidate.plugin, &install_dir)?;
    updated.source = install_source(&candidate.source, &candidate.plugin);
    updated.updated_at_ms = Utc::now().timestamp_millis();
    updated.file_manifest = file_manifest;
    let plugin = installed_plugin(candidate.plugin, &updated, enabled);
    Ok(UpdateActivation {
        plugin,
        install: updated,
        backup_dir: backup_dir.exists().then_some(backup_dir),
    })
}

pub(crate) fn commit_update_activation(activation: &UpdateActivation) -> TalosResult<()> {
    if let Some(backup_dir) = &activation.backup_dir {
        if backup_dir.exists() {
            fs::remove_dir_all(backup_dir).map_err(|err| {
                TalosError::StorageError(format!(
                    "failed to remove plugin update backup `{}`: {err}",
                    backup_dir.display()
                ))
            })?;
        }
    }
    Ok(())
}

pub(crate) fn rollback_update_activation(
    state: &PluginStateStore,
    activation: &UpdateActivation,
) -> TalosResult<()> {
    let install_dir = state.install_dir(&activation.install.id);
    if install_dir.exists() {
        fs::remove_dir_all(&install_dir).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to remove failed plugin update `{}`: {err}",
                install_dir.display()
            ))
        })?;
    }
    if let Some(backup_dir) = &activation.backup_dir {
        if backup_dir.exists() {
            fs::rename(backup_dir, &install_dir).map_err(|err| {
                TalosError::StorageError(format!(
                    "failed to restore plugin update backup `{}`: {err}",
                    backup_dir.display()
                ))
            })?;
        }
    }
    Ok(())
}

pub(crate) fn stage_uninstall_plugin(
    state: &PluginStateStore,
    install_id: &PluginInstallId,
) -> TalosResult<UninstallActivation> {
    let path = state.install_dir(install_id);
    let trash_dir = state.install_trash_dir(install_id);
    if trash_dir.exists() {
        fs::remove_dir_all(&trash_dir).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to clear plugin uninstall trash `{}`: {err}",
                trash_dir.display()
            ))
        })?;
    }
    if !path.exists() {
        return Ok(UninstallActivation {
            removed_path: false,
            install_id: install_id.clone(),
            trash_dir,
        });
    }
    fs::rename(&path, &trash_dir).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to stage plugin uninstall `{}`: {err}",
            path.display()
        ))
    })?;
    Ok(UninstallActivation {
        removed_path: true,
        install_id: install_id.clone(),
        trash_dir,
    })
}

pub(crate) fn commit_uninstall_activation(activation: &UninstallActivation) -> TalosResult<()> {
    if activation.removed_path && activation.trash_dir.exists() {
        fs::remove_dir_all(&activation.trash_dir).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to remove plugin uninstall trash `{}`: {err}",
                activation.trash_dir.display()
            ))
        })?;
    }
    Ok(())
}

pub(crate) fn rollback_uninstall_activation(
    state: &PluginStateStore,
    activation: &UninstallActivation,
) -> TalosResult<()> {
    if !activation.removed_path || !activation.trash_dir.exists() {
        return Ok(());
    }
    let install_dir = state.install_dir(&activation.install_id);
    if install_dir.exists() {
        fs::remove_dir_all(&activation.trash_dir).map_err(|err| {
            TalosError::StorageError(format!(
                "failed to remove plugin uninstall trash `{}`: {err}",
                activation.trash_dir.display()
            ))
        })?;
        return Ok(());
    }
    fs::rename(&activation.trash_dir, &install_dir).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to restore staged plugin uninstall `{}`: {err}",
            activation.trash_dir.display()
        ))
    })
}

fn ensure_installable_plugin(plugin: &Plugin) -> TalosResult<()> {
    if plugin.kind == PluginKind::Skill && matches!(plugin.definition, PluginDefinition::Skill(_)) {
        return Ok(());
    }
    if plugin.kind == PluginKind::McpServer
        && matches!(plugin.definition, PluginDefinition::McpServer(_))
    {
        return Ok(());
    }
    Err(TalosError::ValidationError(format!(
        "plugin `{}` is not an installable plugin",
        plugin.id.0
    )))
}

fn plugin_source_dir(
    state: &PluginStateStore,
    source: &MarketplaceSource,
    plugin: &Plugin,
) -> TalosResult<PathBuf> {
    let path_in_source = plugin.path_in_source.as_deref().ok_or_else(|| {
        TalosError::ValidationError(format!("plugin `{}` has no source path", plugin.id.0))
    })?;
    let root = match &source.location {
        MarketplaceSourceLocation::LocalFolder { path } => path.clone(),
        MarketplaceSourceLocation::Git { .. } => {
            state.marketplace_cache_dir(&source.id).join("repo")
        }
    };
    let path = root.join(path_in_source);
    ensure_contained_dir(&root, &path)?;
    let required_file = match plugin.kind {
        PluginKind::Skill => "SKILL.md",
        PluginKind::McpServer => "talos-mcp.json",
        PluginKind::Package | PluginKind::Feature => {
            return Err(TalosError::ValidationError(format!(
                "plugin `{}` is not installable",
                plugin.id.0
            )));
        }
    };
    if is_regular_file(&path.join(required_file)) {
        return Ok(path);
    }
    Err(TalosError::ResourceNotFound(format!(
        "plugin source path `{}` is not materialized",
        path.display()
    )))
}

fn ensure_contained_dir(root: &Path, path: &Path) -> TalosResult<()> {
    let root = fs::canonicalize(root).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to canonicalize plugin source root `{}`: {err}",
            root.display()
        ))
    })?;
    let path = fs::canonicalize(path).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to canonicalize plugin source path `{}`: {err}",
            path.display()
        ))
    })?;
    if path.is_dir() && path.starts_with(&root) {
        return Ok(());
    }
    Err(TalosError::ValidationError(format!(
        "plugin source path `{}` escaped root `{}`",
        path.display(),
        root.display()
    )))
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file())
        .unwrap_or(false)
}

fn materialize_install_dir(
    state: &PluginStateStore,
    candidate: &InstallCandidate,
    install_id: &PluginInstallId,
    install_dir: &Path,
) -> TalosResult<FileManifest> {
    let source_dir = plugin_source_dir(state, &candidate.source, &candidate.plugin)?;
    let target_dir = installed_content_dir_for_kind(
        candidate.plugin.kind,
        install_dir,
        &candidate.plugin.metadata.name,
    )?;
    fs::create_dir_all(&target_dir).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to create plugin install dir `{}`: {err}",
            target_dir.display()
        ))
    })?;
    let manifest = copy_dir(&source_dir, &target_dir)?;
    let required_file = match candidate.plugin.kind {
        PluginKind::Skill => "SKILL.md",
        PluginKind::McpServer => "talos-mcp.json",
        PluginKind::Package | PluginKind::Feature => unreachable!("kind checked above"),
    };
    if !target_dir.join(required_file).is_file() {
        return Err(TalosError::StorageError(format!(
            "plugin install `{}` did not materialize {required_file}",
            install_id.0
        )));
    }
    Ok(manifest)
}

fn install_source(source: &MarketplaceSource, plugin: &Plugin) -> PluginInstallSource {
    match &source.location {
        MarketplaceSourceLocation::Git { url, reference } => PluginInstallSource::Marketplace {
            source_id: source.id.clone(),
            url: url.clone(),
            reference: reference.clone(),
            commit: plugin.source_commit.clone().unwrap_or_default(),
            path_in_source: plugin.path_in_source.clone().unwrap_or_default(),
        },
        MarketplaceSourceLocation::LocalFolder { path } => PluginInstallSource::LocalPath {
            source_id: source.id.clone(),
            path: path.clone(),
            path_in_source: plugin.path_in_source.clone().unwrap_or_default(),
        },
    }
}

fn copy_dir(source: &Path, target: &Path) -> TalosResult<FileManifest> {
    let mut files = Vec::new();
    copy_dir_inner(source, source, target, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(FileManifest { files })
}

pub(crate) fn manifest_for_dir(root: &Path) -> TalosResult<FileManifest> {
    if !root.exists() {
        return Ok(FileManifest::default());
    }
    let mut files = Vec::new();
    manifest_for_dir_inner(root, root, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(FileManifest { files })
}

fn manifest_for_dir_inner(
    root: &Path,
    current: &Path,
    files: &mut Vec<FileEntry>,
) -> TalosResult<()> {
    for entry in fs::read_dir(current).map_err(|err| {
        TalosError::StorageError(format!("failed to read `{}`: {err}", current.display()))
    })? {
        let entry = entry.map_err(|err| {
            TalosError::StorageError(format!(
                "failed to read `{}` entry: {err}",
                current.display()
            ))
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|err| {
            TalosError::StorageError(format!("failed to inspect `{}`: {err}", path.display()))
        })?;
        if file_type.is_dir() {
            manifest_for_dir_inner(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).map_err(|err| {
                TalosError::InternalError(format!(
                    "failed to build plugin file manifest for `{}`: {err}",
                    path.display()
                ))
            })?;
            let bytes = fs::read(&path).map_err(|err| {
                TalosError::StorageError(format!("failed to hash `{}`: {err}", path.display()))
            })?;
            files.push(FileEntry {
                relative_path: relative.to_string_lossy().replace('\\', "/"),
                sha256: sha256_hex(&bytes),
                size_bytes: bytes.len() as u64,
            });
        }
    }
    Ok(())
}

fn has_local_changes(install: &PluginInstall) -> TalosResult<bool> {
    let current = manifest_for_dir(&installed_content_dir(install)?)?;
    Ok(current != install.file_manifest)
}

fn exposed_skill_root_for(plugin: &Plugin, install_dir: &Path) -> Option<PathBuf> {
    (plugin.kind == PluginKind::Skill).then(|| install_dir.join("skills"))
}

fn exposed_mcp_servers_for(
    plugin: &Plugin,
    install_dir: &Path,
) -> TalosResult<Vec<talos_types::McpServerDefinition>> {
    let PluginDefinition::McpServer(mcp) = &plugin.definition else {
        return Ok(Vec::new());
    };
    let mut server = mcp.server.clone();
    let plugin_dir = install_dir.join("mcp").join(&plugin.metadata.name);
    if let talos_types::McpTransportDefinition::Stdio { cwd, .. } = &mut server.transport {
        let resolved = match cwd.take() {
            Some(path) if path.is_absolute() => path,
            Some(path) => plugin_dir.join(path),
            None => plugin_dir,
        };
        *cwd = Some(resolved);
    }
    Ok(vec![server])
}

fn installed_content_dir(install: &PluginInstall) -> TalosResult<PathBuf> {
    installed_content_dir_for_kind(install.kind, &install.installed_path, &install.name)
}

fn installed_content_dir_for_kind(
    kind: PluginKind,
    install_dir: &Path,
    name: &str,
) -> TalosResult<PathBuf> {
    match kind {
        PluginKind::Skill => Ok(install_dir.join("skills").join(name)),
        PluginKind::McpServer => Ok(install_dir.join("mcp").join(name)),
        PluginKind::Package | PluginKind::Feature => Err(TalosError::ValidationError(format!(
            "plugin kind `{kind:?}` is not installable"
        ))),
    }
}

fn copy_dir_inner(
    root: &Path,
    current: &Path,
    target_root: &Path,
    files: &mut Vec<FileEntry>,
) -> TalosResult<()> {
    for entry in fs::read_dir(current).map_err(|err| {
        TalosError::StorageError(format!("failed to read `{}`: {err}", current.display()))
    })? {
        let entry = entry.map_err(|err| {
            TalosError::StorageError(format!(
                "failed to read `{}` entry: {err}",
                current.display()
            ))
        })?;
        let source_path = entry.path();
        let file_type = entry.file_type().map_err(|err| {
            TalosError::StorageError(format!(
                "failed to inspect `{}`: {err}",
                source_path.display()
            ))
        })?;
        let relative = source_path.strip_prefix(root).map_err(|err| {
            TalosError::InternalError(format!(
                "failed to build plugin file manifest for `{}`: {err}",
                source_path.display()
            ))
        })?;
        let target_path = target_root.join(relative);
        if file_type.is_dir() {
            fs::create_dir_all(&target_path).map_err(|err| {
                TalosError::StorageError(format!(
                    "failed to create `{}`: {err}",
                    target_path.display()
                ))
            })?;
            copy_dir_inner(root, &source_path, target_root, files)?;
        } else if file_type.is_file() {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    TalosError::StorageError(format!(
                        "failed to create `{}`: {err}",
                        parent.display()
                    ))
                })?;
            }
            fs::copy(&source_path, &target_path).map_err(|err| {
                TalosError::StorageError(format!(
                    "failed to copy `{}` to `{}`: {err}",
                    source_path.display(),
                    target_path.display()
                ))
            })?;
            let bytes = fs::read(&target_path).map_err(|err| {
                TalosError::StorageError(format!(
                    "failed to hash `{}`: {err}",
                    target_path.display()
                ))
            })?;
            files.push(FileEntry {
                relative_path: relative.to_string_lossy().replace('\\', "/"),
                sha256: sha256_hex(&bytes),
                size_bytes: bytes.len() as u64,
            });
        }
    }
    Ok(())
}

fn install_id_for(plugin_id: &PluginId) -> PluginInstallId {
    let mut hasher = Sha256::new();
    hasher.update(plugin_id.0.as_bytes());
    let digest = hasher.finalize();
    PluginInstallId(format!("install_{}", short_hex(&digest[..12])))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    short_hex(&digest)
}

fn short_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
