use crate::documents::{is_regular_file, relative_path};
use std::fs;
use std::path::{Path, PathBuf};
use talos_error::{TalosError, TalosResult};
use talos_types::{MarketplaceSource, PluginKind, PluginWarning, PluginWarningCode};

pub(crate) fn scan_roots(source: &MarketplaceSource, root: &Path) -> Vec<PathBuf> {
    if source.sparse_paths.is_empty() {
        return vec![root.to_path_buf()];
    }
    source
        .sparse_paths
        .iter()
        .map(|path| root.join(path))
        .collect()
}

pub(crate) fn ensure_contained_dir(root: &Path, path: &Path) -> TalosResult<()> {
    let root = fs::canonicalize(root).map_err(|err| {
        TalosError::internal(format!(
            "failed to canonicalize plugin source root `{}`: {err}",
            root.display()
        ))
    })?;
    let path = fs::canonicalize(path).map_err(|err| {
        TalosError::internal(format!(
            "failed to canonicalize plugin source path `{}`: {err}",
            path.display()
        ))
    })?;
    if path.is_dir() && path.starts_with(&root) {
        return Ok(());
    }
    Err(TalosError::public(
        talos_error::ErrorCode::InvalidRequest,
        format!(
            "plugin source path `{}` escaped root `{}`",
            path.display(),
            root.display()
        ),
    ))
}

pub(crate) fn mcp_manifest_paths(
    source: &MarketplaceSource,
    source_root: &Path,
    scan_root: &Path,
    warnings: &mut Vec<PluginWarning>,
) -> TalosResult<Vec<PathBuf>> {
    let mut manifests = Vec::new();
    collect_mcp_manifest_paths(
        source,
        source_root,
        scan_root,
        None,
        &mut manifests,
        warnings,
    )?;
    manifests.sort();
    Ok(manifests)
}

pub(crate) fn skill_dirs(
    source: &MarketplaceSource,
    source_root: &Path,
    scan_root: &Path,
    warnings: &mut Vec<PluginWarning>,
) -> TalosResult<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    collect_skill_dirs(source, source_root, scan_root, None, &mut dirs, warnings)?;
    dirs.sort();
    Ok(dirs)
}

fn collect_mcp_manifest_paths(
    source: &MarketplaceSource,
    source_root: &Path,
    current: &Path,
    ancestor_mcp_dir: Option<&Path>,
    manifests: &mut Vec<PathBuf>,
    warnings: &mut Vec<PluginWarning>,
) -> TalosResult<()> {
    let manifest_path = current.join("talos-mcp.json");
    let has_manifest = is_regular_file(&manifest_path);
    if has_manifest {
        if let Some(ancestor) = ancestor_mcp_dir {
            let path_in_source = relative_path(source_root, current)?;
            warnings.push(PluginWarning {
                code: PluginWarningCode::NestedPlugin,
                message: format!(
                    "MCP plugin `{}` is nested under MCP plugin `{}`",
                    path_in_source,
                    relative_path(source_root, ancestor)?
                ),
                kind: Some(PluginKind::McpServer),
                source_id: Some(source.id.clone()),
                path: Some(path_in_source),
            });
        }
        manifests.push(manifest_path);
    }

    let next_ancestor = if has_manifest {
        Some(current)
    } else {
        ancestor_mcp_dir
    };
    for child in child_dirs(current)? {
        collect_mcp_manifest_paths(
            source,
            source_root,
            &child,
            next_ancestor,
            manifests,
            warnings,
        )?;
    }
    Ok(())
}

fn collect_skill_dirs(
    source: &MarketplaceSource,
    source_root: &Path,
    current: &Path,
    ancestor_skill: Option<&Path>,
    dirs: &mut Vec<PathBuf>,
    warnings: &mut Vec<PluginWarning>,
) -> TalosResult<()> {
    let has_skill = is_regular_file(&current.join("SKILL.md"));
    if has_skill {
        if let Some(ancestor) = ancestor_skill {
            let path_in_source = relative_path(source_root, current)?;
            warnings.push(PluginWarning {
                code: PluginWarningCode::NestedPlugin,
                message: format!(
                    "skill `{}` is nested under skill `{}`",
                    path_in_source,
                    relative_path(source_root, ancestor)?
                ),
                kind: Some(PluginKind::Skill),
                source_id: Some(source.id.clone()),
                path: Some(path_in_source),
            });
        }
        dirs.push(current.to_path_buf());
    }

    let next_ancestor = if has_skill {
        Some(current)
    } else {
        ancestor_skill
    };
    for child in child_dirs(current)? {
        collect_skill_dirs(source, source_root, &child, next_ancestor, dirs, warnings)?;
    }
    Ok(())
}

fn child_dirs(path: &Path) -> TalosResult<Vec<PathBuf>> {
    let entries = fs::read_dir(path).map_err(|err| {
        TalosError::internal(format!(
            "failed to read plugin source path `{}`: {err}",
            path.display()
        ))
    })?;
    let mut child_dirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| {
            TalosError::internal(format!(
                "failed to read plugin source path `{}` entry: {err}",
                path.display()
            ))
        })?;
        let child_path = entry.path();
        let file_type = entry.file_type().map_err(|err| {
            TalosError::internal(format!(
                "failed to inspect `{}`: {err}",
                child_path.display()
            ))
        })?;
        if file_type.is_dir() {
            child_dirs.push(child_path);
        }
    }
    child_dirs.sort();
    Ok(child_dirs)
}
