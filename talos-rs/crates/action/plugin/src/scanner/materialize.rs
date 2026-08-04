use crate::PluginStateStore;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_fs::FileSystem;
use talos_types::{MarketplaceSource, MarketplaceSourceLocation};
use wait_timeout::ChildExt;

const GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) struct MaterializedSource {
    pub root: PathBuf,
    pub commit: Option<String>,
}

pub(crate) fn materialize_source(
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
                talos_fs::std_fs()
                    .remove_directory(&repo_dir)
                    .map_err(|err| {
                        TalosError::internal(format!(
                            "failed to clear plugin source cache `{}`: {err}",
                            repo_dir.display()
                        ))
                    })?;
            }
            talos_fs::create_directory(&repo_dir, true).map_err(|err| {
                TalosError::internal(format!(
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
    Err(TalosError::public(
        talos_error::ErrorCode::ToolExecutionFailed,
        format!(
            "git {} failed in `{}`: {}",
            args.join(" "),
            cwd.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    ))
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
    Err(TalosError::public(
        talos_error::ErrorCode::ToolExecutionFailed,
        format!(
            "git {} failed in `{}`: {}",
            args.join(" "),
            cwd.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    ))
}

fn run_git_command(args: &[String], cwd: &Path) -> TalosResult<Output> {
    let mut child = git_command(args, cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            TalosError::public(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("failed to run git in `{}`: {err}", cwd.display()),
            )
        })?;
    match child.wait_timeout(GIT_COMMAND_TIMEOUT).map_err(|err| {
        TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            format!("failed to wait for git in `{}`: {err}", cwd.display()),
        )
    })? {
        Some(_) => child.wait_with_output().map_err(|err| {
            TalosError::public(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("failed to collect git output in `{}`: {err}", cwd.display()),
            )
        }),
        None => {
            let _ = child.kill();
            let _ = child.wait();
            Err(TalosError::public(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!(
                    "git {} timed out after {} seconds in `{}`",
                    args.join(" "),
                    GIT_COMMAND_TIMEOUT.as_secs(),
                    cwd.display()
                ),
            ))
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
