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
use talos_fs::FileSystem;
use talos_types::{
    ContentDigest, ContentDigestAlgorithm, MarketplaceSource, MarketplaceSourceLocation, Plugin,
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
    previous_installed_path: PathBuf,
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
    enabled: bool,
) -> TalosResult<InstallActivation> {
    ensure_installable_plugin(&candidate.plugin)?;
    let install_id = install_id_for(&candidate.plugin.id);
    let install_dir = final_install_dir(
        state,
        candidate.plugin.kind,
        &candidate.plugin.metadata.name,
    )?;
    let backup_dir = state.transaction_backup_dir(&install_id);
    state.cleanup_install_temps()?;
    if backup_dir.exists() {
        remove_plugin_directory(&backup_dir, "clear plugin install backup dir")?;
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

    let temp_dir = state.transaction_temp_dir(&install_id);
    if temp_dir.exists() {
        remove_plugin_directory(&temp_dir, "clear plugin install temp dir")?;
    }
    let content_digest = materialize_install_dir(state, &candidate, &install_id, &temp_dir)?;
    if replace_existing {
        rename_plugin_path(
            &install_dir,
            &backup_dir,
            "stage existing plugin install for replace",
        )?;
    }
    rename_plugin_path(&temp_dir, &install_dir, "activate plugin install").map_err(|err| {
        let _ = if backup_dir.exists() {
            rename_plugin_path(&backup_dir, &install_dir, "restore plugin install backup")
        } else {
            Ok(())
        };
        TalosError::StorageError(format!(
            "failed to activate plugin install `{}`: {err}",
            install_dir.display()
        ))
    })?;
    let now_ms = Utc::now().timestamp_millis();
    let install = PluginInstall {
        id: install_id.clone(),
        plugin_id: candidate.plugin.id.clone(),
        kind: candidate.plugin.kind,
        name: candidate.plugin.metadata.name.clone(),
        installed_path: install_dir,
        enabled,
        source: install_source(&candidate.source, &candidate.plugin),
        content_digest,
        installed_at_ms: now_ms,
        updated_at_ms: now_ms,
    };
    Ok(InstallActivation {
        install,
        backup_dir: backup_dir.exists().then_some(backup_dir),
    })
}

pub(crate) fn commit_install_activation(activation: &InstallActivation) -> TalosResult<()> {
    if let Some(backup_dir) = &activation.backup_dir {
        if backup_dir.exists() {
            remove_plugin_directory(backup_dir, "remove plugin install backup")?;
        }
    }
    Ok(())
}

pub(crate) fn rollback_install_activation(
    _state: &PluginStateStore,
    activation: &InstallActivation,
) -> TalosResult<()> {
    let install_dir = &activation.install.installed_path;
    if install_dir.exists() {
        remove_plugin_directory(install_dir, "remove failed plugin install")?;
    }
    if let Some(backup_dir) = &activation.backup_dir {
        if backup_dir.exists() {
            rename_plugin_path(backup_dir, install_dir, "restore plugin install backup")?;
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

    let install_dir = final_install_dir(
        state,
        candidate.plugin.kind,
        &candidate.plugin.metadata.name,
    )?;
    let temp_dir = state.transaction_temp_dir(&install.id);
    let backup_dir = state.transaction_backup_dir(&install.id);
    if temp_dir.exists() {
        remove_plugin_directory(&temp_dir, "clear plugin update temp dir")?;
    }
    if backup_dir.exists() {
        remove_plugin_directory(&backup_dir, "clear plugin update backup dir")?;
    }
    let content_digest = materialize_install_dir(state, &candidate, &install.id, &temp_dir)?;
    if install.installed_path.exists() {
        rename_plugin_path(
            &install.installed_path,
            &backup_dir,
            "stage plugin install for update",
        )?;
    }
    rename_plugin_path(&temp_dir, &install_dir, "activate plugin update").map_err(|err| {
        let _ = if backup_dir.exists() {
            rename_plugin_path(
                &backup_dir,
                &install.installed_path,
                "restore plugin update backup",
            )
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
    updated.enabled = enabled;
    updated.source = install_source(&candidate.source, &candidate.plugin);
    updated.updated_at_ms = Utc::now().timestamp_millis();
    updated.content_digest = content_digest;
    let plugin = installed_plugin(candidate.plugin, &updated, enabled);
    Ok(UpdateActivation {
        plugin,
        install: updated,
        backup_dir: backup_dir.exists().then_some(backup_dir),
        previous_installed_path: install.installed_path.clone(),
    })
}

pub(crate) fn commit_update_activation(activation: &UpdateActivation) -> TalosResult<()> {
    if let Some(backup_dir) = &activation.backup_dir {
        if backup_dir.exists() {
            remove_plugin_directory(backup_dir, "remove plugin update backup")?;
        }
    }
    Ok(())
}

pub(crate) fn rollback_update_activation(
    _state: &PluginStateStore,
    activation: &UpdateActivation,
) -> TalosResult<()> {
    if activation.install.installed_path.exists() {
        remove_plugin_directory(
            &activation.install.installed_path,
            "remove failed plugin update",
        )?;
    }
    if let Some(backup_dir) = &activation.backup_dir {
        if backup_dir.exists() {
            rename_plugin_path(
                backup_dir,
                &activation.previous_installed_path,
                "restore plugin update backup",
            )?;
        }
    }
    Ok(())
}

pub(crate) fn stage_uninstall_plugin(
    state: &PluginStateStore,
    install: &PluginInstall,
) -> TalosResult<UninstallActivation> {
    let path = install.installed_path.clone();
    let trash_dir = state.transaction_trash_dir(&install.id);
    if trash_dir.exists() {
        remove_plugin_directory(&trash_dir, "clear plugin uninstall trash")?;
    }
    if !path.exists() {
        return Ok(UninstallActivation {
            removed_path: false,
            install_id: install.id.clone(),
            trash_dir,
        });
    }
    rename_plugin_path(&path, &trash_dir, "stage plugin uninstall")?;
    Ok(UninstallActivation {
        removed_path: true,
        install_id: install.id.clone(),
        trash_dir,
    })
}

pub(crate) fn commit_uninstall_activation(activation: &UninstallActivation) -> TalosResult<()> {
    if activation.removed_path && activation.trash_dir.exists() {
        remove_plugin_directory(&activation.trash_dir, "remove plugin uninstall trash")?;
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
    let install = state
        .load_installs()?
        .installs
        .into_iter()
        .find(|install| install.id == activation.install_id);
    let Some(install) = install else {
        remove_plugin_directory(&activation.trash_dir, "remove plugin uninstall trash")?;
        return Ok(());
    };
    let install_dir = install.installed_path;
    if install_dir.exists() {
        remove_plugin_directory(&activation.trash_dir, "remove plugin uninstall trash")?;
        return Ok(());
    }
    rename_plugin_path(
        &activation.trash_dir,
        &install_dir,
        "restore staged plugin uninstall",
    )
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

fn remove_plugin_directory(path: &Path, action: &str) -> TalosResult<()> {
    talos_fs::std_fs().remove_directory(path).map_err(|err| {
        TalosError::StorageError(format!("failed to {action} `{}`: {err}", path.display()))
    })
}

fn create_plugin_directory(path: &Path, action: &str) -> TalosResult<()> {
    talos_fs::create_directory(path, true).map_err(|err| {
        TalosError::StorageError(format!("failed to {action} `{}`: {err}", path.display()))
    })
}

fn rename_plugin_path(source: &Path, target: &Path, action: &str) -> TalosResult<()> {
    if let Some(parent) = target.parent() {
        create_plugin_directory(parent, "create plugin transaction parent")?;
    }
    talos_fs::rename(source, target).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to {action} `{}` to `{}`: {err}",
            source.display(),
            target.display()
        ))
    })
}

fn copy_plugin_file(source: &Path, target: &Path) -> TalosResult<()> {
    talos_fs::copy_file(source, target)
        .map(|_| ())
        .map_err(|err| {
            TalosError::StorageError(format!(
                "failed to copy `{}` to `{}`: {err}",
                source.display(),
                target.display()
            ))
        })
}

fn materialize_install_dir(
    state: &PluginStateStore,
    candidate: &InstallCandidate,
    install_id: &PluginInstallId,
    install_dir: &Path,
) -> TalosResult<ContentDigest> {
    let source_dir = plugin_source_dir(state, &candidate.source, &candidate.plugin)?;
    create_plugin_directory(install_dir, "create plugin install dir")?;
    copy_dir(&source_dir, install_dir)?;
    let required_file = match candidate.plugin.kind {
        PluginKind::Skill => "SKILL.md",
        PluginKind::McpServer => "talos-mcp.json",
        PluginKind::Package | PluginKind::Feature => unreachable!("kind checked above"),
    };
    if !install_dir.join(required_file).is_file() {
        return Err(TalosError::StorageError(format!(
            "plugin install `{}` did not materialize {required_file}",
            install_id.0
        )));
    }
    content_digest_for_dir(install_dir)
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

fn has_local_changes(install: &PluginInstall) -> TalosResult<bool> {
    let current = content_digest_for_dir(&install.installed_path)?;
    Ok(current != install.content_digest)
}

fn final_install_dir(
    state: &PluginStateStore,
    kind: PluginKind,
    name: &str,
) -> TalosResult<PathBuf> {
    validate_install_name(name)?;
    match kind {
        PluginKind::Skill => Ok(state.skill_installs_dir().join(name)),
        PluginKind::McpServer => Ok(state.mcp_installs_dir().join(name)),
        PluginKind::Package | PluginKind::Feature => Err(TalosError::ValidationError(format!(
            "plugin kind `{kind:?}` is not installable"
        ))),
    }
}

fn validate_install_name(name: &str) -> TalosResult<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(TalosError::ValidationError(format!(
            "invalid plugin install name `{name}`"
        )));
    }
    Ok(())
}

fn copy_dir(source: &Path, target: &Path) -> TalosResult<()> {
    copy_dir_inner(source, source, target)
}

fn copy_dir_inner(root: &Path, current: &Path, target_root: &Path) -> TalosResult<()> {
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
            create_plugin_directory(&target_path, "create plugin install child dir")?;
            copy_dir_inner(root, &source_path, target_root)?;
        } else if file_type.is_file() {
            copy_plugin_file(&source_path, &target_path)?;
        }
    }
    Ok(())
}

pub(crate) fn content_digest_for_dir(root: &Path) -> TalosResult<ContentDigest> {
    let mut files = Vec::new();
    collect_digest_files(root, root, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    let mut aggregate = Sha256::new();
    let mut total_size_bytes = 0_u64;
    for file in &files {
        aggregate.update(file.relative_path.as_bytes());
        aggregate.update(b"\0");
        aggregate.update(file.size_bytes.to_le_bytes());
        aggregate.update(b"\0");
        aggregate.update(file.content_sha256.as_bytes());
        aggregate.update(b"\0");
        total_size_bytes += file.size_bytes;
    }

    Ok(ContentDigest {
        algorithm: ContentDigestAlgorithm::Sha256,
        value: format!("sha256:{}", short_hex(&aggregate.finalize())),
        file_count: files.len() as u64,
        total_size_bytes,
    })
}

struct DigestFile {
    relative_path: String,
    content_sha256: String,
    size_bytes: u64,
}

fn collect_digest_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<DigestFile>,
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
            collect_digest_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).map_err(|err| {
                TalosError::InternalError(format!(
                    "failed to build plugin content digest path for `{}`: {err}",
                    path.display()
                ))
            })?;
            let bytes = fs::read(&path).map_err(|err| {
                TalosError::StorageError(format!("failed to hash `{}`: {err}", path.display()))
            })?;
            files.push(DigestFile {
                relative_path: relative.to_string_lossy().replace('\\', "/"),
                content_sha256: sha256_hex(&bytes),
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
