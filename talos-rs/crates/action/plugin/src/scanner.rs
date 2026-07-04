mod discovery;
mod materialize;
pub(crate) mod mcp_manifest;
mod skill_manifest;

use crate::documents::relative_path;
use crate::PluginStateStore;
use chrono::Utc;
use std::path::Path;
use talos_error::TalosResult;
use talos_types::{
    MarketplaceSource, Plugin, PluginKind, PluginWarning, PluginWarningCode, SkillLoadError,
    SkillLoadErrorKind,
};

use discovery::{ensure_contained_dir, mcp_manifest_paths, scan_roots, skill_dirs};
use materialize::materialize_source;
use mcp_manifest::plugin_from_mcp_manifest;
use skill_manifest::plugin_from_skill_dir;

pub(crate) struct SourceScanResult {
    pub source_commit: Option<String>,
    pub plugins: Vec<Plugin>,
    pub warnings: Vec<PluginWarning>,
}

pub(crate) fn scan_source(
    state: &PluginStateStore,
    source: &MarketplaceSource,
) -> TalosResult<SourceScanResult> {
    let materialized = materialize_source(state, source)?;
    let (plugins, warnings) = scan_materialized_plugins(source, &materialized.root)?;
    Ok(SourceScanResult {
        source_commit: materialized.commit,
        plugins,
        warnings,
    })
}

fn scan_materialized_plugins(
    source: &MarketplaceSource,
    root: &Path,
) -> TalosResult<(Vec<Plugin>, Vec<PluginWarning>)> {
    let mut plugins = Vec::new();
    let mut warnings = Vec::new();
    for scan_root in scan_roots(source, root) {
        if !scan_root.exists() {
            continue;
        }
        ensure_contained_dir(root, &scan_root)?;
        let mut scan_warnings = Vec::new();
        for skill_dir in skill_dirs(source, root, &scan_root, &mut scan_warnings)? {
            match plugin_from_skill_dir(source, root, &scan_root, &skill_dir)? {
                Ok(plugin) => plugins.push(plugin),
                Err(error) => warnings.push(skill_load_warning(source, root, &skill_dir, &error)),
            }
        }
        for manifest_path in mcp_manifest_paths(source, root, &scan_root, &mut scan_warnings)? {
            match plugin_from_mcp_manifest(source, root, &manifest_path) {
                Ok(plugin) => plugins.push(plugin),
                Err(error) => {
                    warnings.push(mcp_load_warning(
                        source,
                        root,
                        &manifest_path,
                        &error.to_string(),
                    ));
                }
            }
        }
        warnings.append(&mut scan_warnings);
    }
    plugins.sort_by(|left, right| {
        left.metadata
            .name
            .cmp(&right.metadata.name)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok((plugins, warnings))
}

fn skill_load_warning(
    source: &MarketplaceSource,
    root: &Path,
    skill_dir: &Path,
    error: &SkillLoadError,
) -> PluginWarning {
    let code = skill_load_warning_code(error.kind);
    let path = relative_path(root, skill_dir).unwrap_or_else(|_| skill_dir.display().to_string());
    PluginWarning {
        code,
        message: format!("failed to load skill `{path}`: {}", error.message),
        kind: Some(PluginKind::Skill),
        source_id: Some(source.id.clone()),
        path: Some(path),
    }
}

fn skill_load_warning_code(kind: SkillLoadErrorKind) -> PluginWarningCode {
    match kind {
        SkillLoadErrorKind::MissingRequiredField => PluginWarningCode::MissingRequiredField,
        SkillLoadErrorKind::SkillRootReadFailed
        | SkillLoadErrorKind::SkillDirectoryReadFailed
        | SkillLoadErrorKind::MissingSkillFile
        | SkillLoadErrorKind::SkillFileMetadataReadFailed
        | SkillLoadErrorKind::SkillFileTooLarge
        | SkillLoadErrorKind::SkillFileReadFailed
        | SkillLoadErrorKind::InvalidFrontmatter => PluginWarningCode::InvalidManifest,
    }
}

fn mcp_load_warning(
    source: &MarketplaceSource,
    root: &Path,
    manifest_path: &Path,
    message: &str,
) -> PluginWarning {
    let path =
        relative_path(root, manifest_path).unwrap_or_else(|_| manifest_path.display().to_string());
    PluginWarning {
        code: PluginWarningCode::InvalidManifest,
        message: format!("failed to load MCP plugin `{path}`: {message}"),
        kind: Some(PluginKind::McpServer),
        source_id: Some(source.id.clone()),
        path: Some(path),
    }
}

pub(crate) fn scanned_at_ms() -> i64 {
    Utc::now().timestamp_millis()
}
