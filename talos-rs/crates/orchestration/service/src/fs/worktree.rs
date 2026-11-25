//! Node-local verification and creation of a checkout for a Session.
use agent_protocol::WorkerWorktreeRequest;
use std::path::Path;
use std::time::Duration;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::SessionWorktreeBinding;
use tokio::{fs, process::Command};

pub(crate) async fn resolve(request: WorkerWorktreeRequest) -> TalosResult<SessionWorktreeBinding> {
    match request {
        WorkerWorktreeRequest::Bind {
            repository,
            checkout,
        } => {
            let binding = linked_checkout(&checkout).await?;
            if let Some(repository) = repository {
                let source = git_identity(&repository).await?;
                if source.git_common_directory != binding.git_common_directory {
                    return Err(TalosError::authorization_denied());
                }
            }
            Ok(binding)
        }
        WorkerWorktreeRequest::CreateFromOriginMain {
            repository,
            operation_id,
            expected_common_directory,
        } => {
            create(
                &repository,
                operation_id,
                expected_common_directory.as_deref(),
            )
            .await
        }
        WorkerWorktreeRequest::Verify(expected) => {
            let binding = linked_checkout(&expected.checkout_root).await?;
            if expected != binding {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "The Session worktree no longer matches the bound Git checkout.",
                ));
            }
            Ok(binding)
        }
    }
}

async fn create(
    repository: &Path,
    operation_id: uuid::Uuid,
    expected_common_directory: Option<&Path>,
) -> TalosResult<SessionWorktreeBinding> {
    let source = git_identity(repository).await?;
    if expected_common_directory.is_some_and(|expected| expected != source.git_common_directory) {
        return Err(TalosError::authorization_denied());
    }
    let entries = git(
        &source.checkout_root,
        &["worktree", "list", "--porcelain", "-z"],
    )
    .await?;
    let primary = entries
        .split('\0')
        .next()
        .and_then(|field| field.strip_prefix("worktree "))
        .ok_or_else(|| TalosError::internal("Git did not return the primary checkout."))?;
    let primary = git_identity(Path::new(primary)).await?;
    if source.git_common_directory != primary.git_common_directory
        || primary.git_common_directory != primary.git_directory
    {
        return Err(TalosError::authorization_denied());
    }
    let parent = primary.checkout_root.parent().ok_or_else(|| {
        TalosError::new(
            ErrorCode::InvalidRequest,
            "The repository has no parent directory for worktrees.",
        )
    })?;
    let directory = parent.join(".worktrees");
    match fs::create_dir(&directory).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(path_error(error)),
    }
    if !fs::symlink_metadata(&directory)
        .await
        .map_err(path_error)?
        .is_dir()
    {
        return Err(TalosError::authorization_denied());
    }
    let checkout = directory.join(format!("talos-{}", operation_id.simple()));
    match fs::symlink_metadata(&checkout).await {
        Ok(metadata) => {
            if !metadata.is_dir() {
                return Err(TalosError::authorization_denied());
            }
            let binding = linked_checkout(&checkout).await?;
            if binding.checkout_root != checkout
                || binding.git_common_directory != source.git_common_directory
            {
                return Err(TalosError::authorization_denied());
            }
            return Ok(binding);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(path_error(error)),
    }
    git(
        &source.checkout_root,
        &[
            "fetch",
            "--no-tags",
            "--no-recurse-submodules",
            "--no-write-fetch-head",
            "origin",
            "+refs/heads/main:refs/remotes/origin/main",
        ],
    )
    .await?;
    let commit = git(
        &source.checkout_root,
        &["rev-parse", "--verify", "refs/remotes/origin/main^{commit}"],
    )
    .await?;
    let destination = checkout.to_str().ok_or_else(|| {
        TalosError::new(
            ErrorCode::InvalidRequest,
            "The worktree path must be valid UTF-8.",
        )
    })?;
    git(
        &source.checkout_root,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "worktree",
            "add",
            "--detach",
            "--",
            destination,
            commit.trim_end(),
        ],
    )
    .await?;
    let binding = linked_checkout(&checkout).await?;
    if binding.git_common_directory != source.git_common_directory {
        return Err(TalosError::authorization_denied());
    }
    Ok(binding)
}

async fn linked_checkout(checkout: &Path) -> TalosResult<SessionWorktreeBinding> {
    let binding = git_identity(checkout).await?;
    let git_file = binding.checkout_root.join(".git");
    if binding.git_directory.parent()
        != Some(binding.git_common_directory.join("worktrees").as_path())
        || !fs::symlink_metadata(&git_file)
            .await
            .map_err(path_error)?
            .is_file()
    {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Select a registered Git worktree checkout.",
        ));
    }
    let backlink = fs::read_to_string(binding.git_directory.join("gitdir"))
        .await
        .map_err(path_error)?;
    let registered = fs::canonicalize(
        binding
            .git_directory
            .join(backlink.trim_end_matches(['\r', '\n'])),
    )
    .await
    .map_err(path_error)?;
    if registered != git_file {
        return Err(TalosError::authorization_denied());
    }
    Ok(binding)
}

async fn git_identity(path: &Path) -> TalosResult<SessionWorktreeBinding> {
    if !path.is_absolute() {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "The worktree directory must be an absolute path.",
        ));
    }
    let directory = fs::canonicalize(path).await.map_err(path_error)?;
    if !fs::metadata(&directory).await.map_err(path_error)?.is_dir() {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "The selected working directory is not a directory.",
        ));
    }
    let root = git(&directory, &["rev-parse", "--show-toplevel"]).await?;
    let checkout_root = fs::canonicalize(root.trim_end_matches(['\r', '\n']))
        .await
        .map_err(path_error)?;
    let git_dir = git(
        &checkout_root,
        &["rev-parse", "--path-format=absolute", "--git-dir"],
    )
    .await?;
    let common_dir = git(
        &checkout_root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .await?;
    Ok(SessionWorktreeBinding {
        checkout_root,
        git_directory: fs::canonicalize(git_dir.trim_end_matches(['\r', '\n']))
            .await
            .map_err(path_error)?,
        git_common_directory: fs::canonicalize(common_dir.trim_end_matches(['\r', '\n']))
            .await
            .map_err(path_error)?,
    })
}

async fn git(directory: &Path, arguments: &[&str]) -> TalosResult<String> {
    let mut command = Command::new("git");
    command
        .current_dir(directory)
        .env_remove("GIT_DIR")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .args(arguments);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output())
        .await
        .map_err(|_| TalosError::new(ErrorCode::Timeout, "The Git worktree operation timed out."))?
        .map_err(|error| {
            TalosError::internal_with_source("Run Git on the execution Worker.", error)
        })?;
    if !output.status.success() {
        let message = match arguments.first() {
            Some(&"fetch") => "Could not fetch main from the origin remote.",
            Some(&"-c") => "Could not create the Git worktree.",
            _ => "Could not resolve a Git checkout on the execution Worker.",
        };
        return Err(TalosError::new(ErrorCode::InvalidRequest, message)
            .context(format!("{message} Git exited with {}", output.status)));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| TalosError::internal_with_source("Decode Git worktree metadata.", error))
}

fn path_error(error: std::io::Error) -> TalosError {
    match error.kind() {
        std::io::ErrorKind::NotFound => TalosError::new(
            ErrorCode::ResourceNotFound,
            "Current working directory missing. The Session worktree is no longer available.",
        ),
        std::io::ErrorKind::PermissionDenied => TalosError::authorization_denied(),
        _ => TalosError::internal_with_source("Resolve the Session worktree directory.", error),
    }
}
