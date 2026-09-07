use globset::Glob;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::ffi::OsStr;
use talos_error::{TalosError, TalosResult};
use talos_tools_sandbox::{FileSystemOperation, RequestedFileSystemAccess};
use talos_types::talos::TalosToolSearchMatch;

use crate::runtimes::fs_read::{normalize_existing_tool_path, resolve_tool_path};
use crate::runtimes::map_sandbox_error;
use crate::ToolContext;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 2_000;
const MAX_WALK_ENTRIES: usize = 100_000;
const MAX_WALK_DIRECTORIES: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobFilesRequest {
    pub pattern: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobFilesResponse {
    pub query: String,
    pub matches: Vec<TalosToolSearchMatch>,
    pub offset: usize,
    pub limit: usize,
    pub total_matches: usize,
    pub more_available: bool,
    pub partial: bool,
}

pub fn requested_file_system_access(
    request: &GlobFilesRequest,
    context: &ToolContext,
) -> TalosResult<Vec<RequestedFileSystemAccess>> {
    let base = if let Some(path) = request.path.as_deref() {
        resolve_tool_path(path, context)?
    } else {
        crate::runtimes::resolve_base_dir(context)?
    };
    let base = normalize_existing_tool_path(&base)?;
    Ok(vec![RequestedFileSystemAccess::new(
        FileSystemOperation::Read,
        vec![base],
    )
    .map_err(|error| {
        TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            error.to_string(),
        )
    })?])
}

pub async fn run(
    request: &GlobFilesRequest,
    context: &ToolContext,
) -> TalosResult<GlobFilesResponse> {
    if request.pattern.trim().is_empty() {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "pattern must not be empty".to_string(),
        ));
    }
    let base = if let Some(path) = request.path.as_deref() {
        resolve_tool_path(path, context)?
    } else {
        crate::runtimes::resolve_base_dir(context)?
    };
    let matcher = Glob::new(&request.pattern)
        .map_err(|err| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("invalid glob pattern: {}", err),
            )
        })?
        .compile_matcher();
    let limit = request.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
    let offset = request.offset.unwrap_or(0);
    let mut paths = Vec::new();
    let mut queue = VecDeque::from([base.clone()]);
    let mut visited_entries = 0usize;
    let mut visited_directories = 1usize;
    let mut partial = false;
    let file_system = context.sandbox_file_system()?;

    while let Some(directory) = queue.pop_front() {
        let entries = match file_system.read_directory(&directory).await {
            Ok(entries) => entries,
            Err(error) if directory == base => {
                return Err(map_sandbox_error(error));
            }
            Err(_) => continue,
        };
        for entry in entries {
            if visited_entries == MAX_WALK_ENTRIES {
                partial = true;
                queue.clear();
                break;
            }
            visited_entries += 1;
            let metadata = match file_system.metadata(&entry.path).await {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata.is_symlink {
                continue;
            }
            if metadata.is_directory {
                if should_descend(&entry.file_name) {
                    if visited_directories == MAX_WALK_DIRECTORIES {
                        partial = true;
                    } else {
                        visited_directories += 1;
                        queue.push_back(entry.path);
                    }
                }
                continue;
            }
            if !metadata.is_file {
                continue;
            }
            let relative = entry.path.strip_prefix(&base).unwrap_or(&entry.path);
            if matcher.is_match(relative) || matcher.is_match(&entry.path) {
                paths.push(relative.display().to_string());
            }
        }
    }

    paths.sort();
    let total_matches = paths.len();
    let selected = paths
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|path| TalosToolSearchMatch {
            path: path.clone(),
            line: 1,
            column: None,
            text: path,
            before: Vec::new(),
            after: Vec::new(),
        })
        .collect::<Vec<_>>();
    let more_available = offset + selected.len() < total_matches;

    Ok(GlobFilesResponse {
        query: request.pattern.clone(),
        matches: selected,
        offset,
        limit,
        total_matches,
        more_available,
        partial,
    })
}

fn should_descend(file_name: &OsStr) -> bool {
    let name = file_name.to_string_lossy();
    !matches!(
        name.as_ref(),
        ".git"
            | ".hg"
            | ".svn"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | ".next"
            | ".turbo"
            | ".cache"
    )
}

#[cfg(test)]
#[path = "fs_glob_tests.rs"]
mod tests;
