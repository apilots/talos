use crate::source::plugin_id_for;
use crate::PluginStateStore;
use chrono::Utc;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_skills::{loader::parse_skill_dir, SkillRoot, SkillState, SkillSummary};
use talos_types::{
    MarketplaceSource, MarketplaceSourceLocation, Plugin, PluginDefinition, PluginKind,
    PluginStatus, PluginWarning, PluginWarningCode, SkillManifest, SkillManifestMetadata,
    SkillPlugin,
};
use wait_timeout::ChildExt;

const MAX_MARKETPLACE_SKILL_MD_BYTES: usize = 512 * 1024;
const GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

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
    let (plugins, warnings) = scan_materialized_skills(source, &materialized.root)?;
    Ok(SourceScanResult {
        source_commit: materialized.commit,
        plugins,
        warnings,
    })
}

struct MaterializedSource {
    root: PathBuf,
    commit: Option<String>,
}

fn materialize_source(
    state: &PluginStateStore,
    source: &MarketplaceSource,
) -> TalosResult<MaterializedSource> {
    match &source.location {
        MarketplaceSourceLocation::LocalFolder { path } => Ok(MaterializedSource {
            root: path.clone(),
            commit: None,
        }),
        MarketplaceSourceLocation::Git { url, reference } => {
            let repo_dir = state.marketplace_cache_dir(&source.id).join("repo");
            if repo_dir.exists() {
                fs::remove_dir_all(&repo_dir).map_err(|err| {
                    TalosError::StorageError(format!(
                        "failed to clear plugin source cache `{}`: {err}",
                        repo_dir.display()
                    ))
                })?;
            }
            fs::create_dir_all(&repo_dir).map_err(|err| {
                TalosError::StorageError(format!(
                    "failed to create plugin source cache `{}`: {err}",
                    repo_dir.display()
                ))
            })?;

            run_git(["init"], &repo_dir)?;
            run_git(["remote", "add", "origin", url.as_str()], &repo_dir)?;
            if !source.sparse_paths.is_empty() {
                run_git(["sparse-checkout", "init", "--cone"], &repo_dir)?;
                let mut args = vec!["sparse-checkout", "set"];
                args.extend(source.sparse_paths.iter().map(String::as_str));
                run_git(args, &repo_dir)?;
            }
            run_git(
                [
                    "fetch",
                    "--depth=1",
                    "--filter=blob:none",
                    "--recurse-submodules=no",
                    "--no-tags",
                    "origin",
                    reference.as_str(),
                ],
                &repo_dir,
            )?;
            run_git(["checkout", "--detach", "FETCH_HEAD"], &repo_dir)?;
            let commit = git_output(["rev-parse", "HEAD"], &repo_dir)?;
            Ok(MaterializedSource {
                root: repo_dir,
                commit: Some(commit.trim().to_string()),
            })
        }
    }
}

fn scan_materialized_skills(
    source: &MarketplaceSource,
    root: &Path,
) -> TalosResult<(Vec<Plugin>, Vec<PluginWarning>)> {
    let scan_roots = if source.sparse_paths.is_empty() {
        vec![root.to_path_buf()]
    } else {
        source
            .sparse_paths
            .iter()
            .map(|path| root.join(path))
            .collect::<Vec<_>>()
    };

    let mut plugins = Vec::new();
    let mut warnings = Vec::new();
    for scan_root in scan_roots {
        if !scan_root.exists() {
            continue;
        }
        ensure_contained_dir(root, &scan_root)?;
        let mut scan_warnings = Vec::new();
        for skill_dir in skill_dirs(source, root, &scan_root, &mut scan_warnings)? {
            let source_root = SkillRoot::configured(&scan_root);
            let state = SkillState::default();
            let summary = match parse_skill_dir(
                &skill_dir,
                &source_root,
                &state,
                MAX_MARKETPLACE_SKILL_MD_BYTES,
            ) {
                Ok(summary) => summary,
                Err(error) => {
                    warnings.push(skill_load_warning(source, root, &skill_dir, &error.message));
                    continue;
                }
            };
            let path_in_source = relative_path(root, &skill_dir)?;
            plugins.push(plugin_from_skill_summary(source, path_in_source, summary));
        }
        warnings.append(&mut scan_warnings);
    }
    plugins.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok((plugins, warnings))
}

fn skill_dirs(
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

    let entries = match fs::read_dir(current) {
        Ok(entries) => entries,
        Err(err) => {
            return Err(TalosError::StorageError(format!(
                "failed to read plugin source path `{}`: {err}",
                current.display()
            )));
        }
    };
    let next_ancestor = if has_skill {
        Some(current)
    } else {
        ancestor_skill
    };
    let mut child_dirs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| {
            TalosError::StorageError(format!(
                "failed to read plugin source path `{}` entry: {err}",
                current.display()
            ))
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|err| {
            TalosError::StorageError(format!("failed to inspect `{}`: {err}", path.display()))
        })?;
        if file_type.is_dir() {
            child_dirs.push(path);
        }
    }
    child_dirs.sort();
    for child in child_dirs {
        collect_skill_dirs(source, source_root, &child, next_ancestor, dirs, warnings)?;
    }
    Ok(())
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

fn plugin_from_skill_summary(
    source: &MarketplaceSource,
    path_in_source: String,
    summary: SkillSummary,
) -> Plugin {
    let skill_md_path = format!("{path_in_source}/SKILL.md");
    let metadata = summary
        .metadata
        .as_ref()
        .map(|metadata| SkillManifestMetadata {
            display_name: metadata.display_name.clone(),
            icon: metadata.icon.clone(),
        });
    let manifest = SkillManifest {
        name: summary.name.clone(),
        description: summary.description.clone(),
        short_description: summary.short_description.clone(),
        metadata,
        license: None,
    };
    Plugin {
        id: plugin_id_for(&source.id, PluginKind::Skill, &path_in_source),
        kind: PluginKind::Skill,
        name: summary.name,
        description: summary.description,
        source_id: Some(source.id.clone()),
        path_in_source: Some(path_in_source),
        source_commit: source.last_checked_commit.clone(),
        status: PluginStatus::NotInstalled,
        definition: PluginDefinition::Skill(SkillPlugin {
            manifest,
            skill_md_path,
        }),
        warnings: Vec::new(),
    }
}

fn relative_path(root: &Path, path: &Path) -> TalosResult<String> {
    path.strip_prefix(root)
        .map_err(|err| {
            TalosError::InternalError(format!(
                "plugin source path `{}` escaped root `{}`: {err}",
                path.display(),
                root.display()
            ))
        })
        .map(|path| path.to_string_lossy().replace('\\', "/"))
}

fn skill_load_warning(
    source: &MarketplaceSource,
    root: &Path,
    skill_dir: &Path,
    message: &str,
) -> PluginWarning {
    let code = if message.contains("missing required frontmatter field") {
        PluginWarningCode::MissingRequiredField
    } else {
        PluginWarningCode::InvalidManifest
    };
    let path = relative_path(root, skill_dir).unwrap_or_else(|_| skill_dir.display().to_string());
    PluginWarning {
        code,
        message: format!("failed to load skill `{path}`: {message}"),
        kind: Some(PluginKind::Skill),
        source_id: Some(source.id.clone()),
        path: Some(path),
    }
}

fn run_git<I, S>(args: I, cwd: &Path) -> TalosResult<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect();
    let output = run_git_command(&args, cwd)?;
    if output.status.success() {
        return Ok(());
    }
    Err(TalosError::ToolError(format!(
        "git {} failed in `{}`: {}",
        args.join(" "),
        cwd.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn git_output<I, S>(args: I, cwd: &Path) -> TalosResult<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string())
        .collect();
    let output = run_git_command(&args, cwd)?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    Err(TalosError::ToolError(format!(
        "git {} failed in `{}`: {}",
        args.join(" "),
        cwd.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn run_git_command(args: &[String], cwd: &Path) -> TalosResult<Output> {
    let mut child = git_command(args, cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            TalosError::ToolError(format!("failed to run git in `{}`: {err}", cwd.display()))
        })?;
    match child.wait_timeout(GIT_COMMAND_TIMEOUT).map_err(|err| {
        TalosError::ToolError(format!(
            "failed to wait for git in `{}`: {err}",
            cwd.display()
        ))
    })? {
        Some(_) => child.wait_with_output().map_err(|err| {
            TalosError::ToolError(format!(
                "failed to collect git output in `{}`: {err}",
                cwd.display()
            ))
        }),
        None => {
            let _ = child.kill();
            let _ = child.wait();
            Err(TalosError::ToolError(format!(
                "git {} timed out after {} seconds in `{}`",
                args.join(" "),
                GIT_COMMAND_TIMEOUT.as_secs(),
                cwd.display()
            )))
        }
    }
}

fn git_command(args: &[String], cwd: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_ALLOW_PROTOCOL", "https:ssh:file");
    command
}

pub(crate) fn scanned_at_ms() -> i64 {
    Utc::now().timestamp_millis()
}
