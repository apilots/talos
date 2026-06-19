use crate::catalog::{read_document, refresh, select_skill};
use crate::loader::parse_skill_dir;
use crate::model::{SkillRoot, SkillRoots, SkillScope, SkillSelector, SkillSummary};
use crate::state::{load_skill_state, save_skill_state, SkillState};
use std::fs;
use std::path::{Path, PathBuf};
use talos_error::{TalosError, TalosResult};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct SkillAdminContext {
    pub roots: SkillRoots,
}

impl SkillAdminContext {
    pub fn new(roots: SkillRoots) -> Self {
        Self { roots }
    }
}

#[derive(Debug, Clone)]
pub enum SkillInstallSource {
    LocalPath(PathBuf),
}

pub fn install_skill(
    ctx: &SkillAdminContext,
    source: SkillInstallSource,
) -> TalosResult<SkillSummary> {
    let SkillInstallSource::LocalPath(source_dir) = source;
    if !source_dir.is_dir() {
        return Err(TalosError::ValidationError(format!(
            "skill install source `{}` is not a directory",
            source_dir.display()
        )));
    }
    let install_root = &ctx.roots.install_root;
    fs::create_dir_all(install_root).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to create skill install root `{}`: {err}",
            install_root.display()
        ))
    })?;
    let empty_state = SkillState::default();
    let source_root = SkillRoot::user(&source_dir);
    let parsed = parse_skill_dir(
        &source_dir,
        &source_root,
        &empty_state,
        ctx.roots.max_skill_md_bytes,
    )
    .map_err(|err| TalosError::ValidationError(err.message))?;
    let destination = install_root.join(&parsed.name);
    if destination.exists() {
        return Err(TalosError::Conflict(format!(
            "skill `{}` already exists at `{}`",
            parsed.name,
            destination.display()
        )));
    }
    if let Some(existing) = refresh(&ctx.roots)?
        .skills
        .into_iter()
        .find(|skill| skill.name == parsed.name)
    {
        return Err(TalosError::Conflict(format!(
            "skill `{}` already exists at `{}`",
            parsed.name,
            existing.path.display()
        )));
    }
    copy_skill_dir(&source_dir, &destination)?;
    let state = load_skill_state(&ctx.roots.state_path)?;
    let installed_root = SkillRoot::user(&destination);
    parse_skill_dir(
        &destination,
        &installed_root,
        &state,
        ctx.roots.max_skill_md_bytes,
    )
    .map_err(|err| TalosError::ValidationError(err.message))
}

pub fn uninstall_skill(ctx: &SkillAdminContext, selector: &SkillSelector) -> TalosResult<()> {
    let (skill, _) = select_skill(&ctx.roots, selector, false)?;
    if skill.scope != SkillScope::User || !is_under(&skill.root, &ctx.roots.install_root) {
        return Err(TalosError::AuthorizationDenied);
    }
    fs::remove_dir_all(&skill.root).map_err(|err| {
        TalosError::StorageError(format!(
            "failed to uninstall skill `{}` at `{}`: {err}",
            skill.name,
            skill.root.display()
        ))
    })?;
    let mut state = load_skill_state(&ctx.roots.state_path)?;
    state.enable(&skill.path);
    save_skill_state(&ctx.roots.state_path, &state)
}

pub fn disable_skill(
    ctx: &SkillAdminContext,
    selector: &SkillSelector,
) -> TalosResult<SkillSummary> {
    let (skill, _) = select_skill(&ctx.roots, selector, false)?;
    let mut state = load_skill_state(&ctx.roots.state_path)?;
    state.disable(skill.path.clone());
    save_skill_state(&ctx.roots.state_path, &state)?;
    read_document(ctx.roots(), &SkillSelector::by_path(skill.path)).map(|doc| doc.summary)
}

pub fn enable_skill(
    ctx: &SkillAdminContext,
    selector: &SkillSelector,
) -> TalosResult<SkillSummary> {
    let (skill, _) = select_skill(&ctx.roots, selector, false)?;
    let mut state = load_skill_state(&ctx.roots.state_path)?;
    state.enable(&skill.path);
    save_skill_state(&ctx.roots.state_path, &state)?;
    read_document(ctx.roots(), &SkillSelector::by_path(skill.path)).map(|doc| doc.summary)
}

impl SkillAdminContext {
    fn roots(&self) -> &SkillRoots {
        &self.roots
    }
}

fn copy_skill_dir(source: &Path, destination: &Path) -> TalosResult<()> {
    for entry in WalkDir::new(source).into_iter().filter_entry(|entry| {
        entry
            .file_name()
            .to_str()
            .map(|name| !is_excluded_name(name))
            .unwrap_or(true)
    }) {
        let entry = entry.map_err(|err| {
            TalosError::StorageError(format!(
                "failed to walk skill directory `{}`: {err}",
                source.display()
            ))
        })?;
        let relative = entry.path().strip_prefix(source).map_err(|err| {
            TalosError::InternalError(format!("failed to compute skill relative path: {err}"))
        })?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).map_err(|err| {
                TalosError::StorageError(format!(
                    "failed to create skill directory `{}`: {err}",
                    target.display()
                ))
            })?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|err| {
                    TalosError::StorageError(format!(
                        "failed to create skill directory `{}`: {err}",
                        parent.display()
                    ))
                })?;
            }
            fs::copy(entry.path(), &target).map_err(|err| {
                TalosError::StorageError(format!(
                    "failed to copy skill file `{}` to `{}`: {err}",
                    entry.path().display(),
                    target.display()
                ))
            })?;
        }
    }
    Ok(())
}

fn is_excluded_name(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".venv" | "node_modules" | "__pycache__" | "target" | "dist"
    )
}

fn is_under(path: &Path, root: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    path.starts_with(root)
}
