use std::path::Path;
use std::process::Command;

pub(super) struct GitRepositoryState {
    pub(super) branch: String,
    pub(super) status_summary: String,
    pub(super) has_uncommitted_changes: bool,
}

pub(super) fn append_project_context(lines: &mut Vec<String>, project_root: &Path) {
    lines.push(format!("- project_root: {}", project_root.display()));
    if std::fs::symlink_metadata(project_root.join(".git")).is_ok_and(|metadata| metadata.is_file())
    {
        lines.push("- The current project root is a linked checkout. When it already satisfies the user's worktree request, perform the development work here and do not create another checkout.".to_string());
    }
    if let Some(repository_state) = inspect_git_repository_state(project_root) {
        lines.push(format!("- git_branch: {}", repository_state.branch));
        lines.push(format!(
            "- git_status_summary: {}",
            repository_state.status_summary
        ));
        lines.push(format!(
            "- has_uncommitted_changes: {}",
            repository_state.has_uncommitted_changes
        ));
    }
}

fn inspect_git_repository_state(project_root: &Path) -> Option<GitRepositoryState> {
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
