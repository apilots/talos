use std::path::Path;
use std::process::Command;

pub(super) struct GitRepositoryState {
    pub(super) branch: String,
    pub(super) status_summary: String,
    pub(super) has_uncommitted_changes: bool,
}

pub(super) fn inspect_git_repository_state(project_root: &Path) -> Option<GitRepositoryState> {
    let branch = git_stdout(
        project_root,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    )
    .or_else(|| git_stdout(project_root, &["rev-parse", "--abbrev-ref", "HEAD"]))?;

    let status_output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["status", "--short"])
        .output()
        .ok()?;
    if !status_output.status.success() {
        return None;
    }

    let entries = String::from_utf8_lossy(&status_output.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    let has_uncommitted_changes = entries > 0;
    let status_summary = if has_uncommitted_changes {
        format!("modified {entries} file(s)")
    } else {
        "clean".to_string()
    };

    Some(GitRepositoryState {
        branch,
        status_summary,
        has_uncommitted_changes,
    })
}

fn git_stdout(project_root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}
