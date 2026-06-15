use globset::Glob;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use talos_types::talos::TalosToolSearchMatch;
use walkdir::{DirEntry, WalkDir};

use crate::runtimes::fs_read::resolve_tool_path;
use crate::ToolContext;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 2_000;

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

pub async fn run(
    request: &GlobFilesRequest,
    context: &ToolContext,
) -> TalosResult<GlobFilesResponse> {
    if request.pattern.trim().is_empty() {
        return Err(TalosError::ToolError(
            "pattern must not be empty".to_string(),
        ));
    }
    let base = if let Some(path) = request.path.as_deref() {
        resolve_tool_path(path, context)?
    } else {
        crate::runtimes::resolve_base_dir(context)?
    };
    let matcher = Glob::new(&request.pattern)
        .map_err(|err| TalosError::ToolError(format!("invalid glob pattern: {}", err)))?
        .compile_matcher();
    let limit = request.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
    let offset = request.offset.unwrap_or(0);
    let mut paths = Vec::new();

    for entry in WalkDir::new(&base)
        .into_iter()
        .filter_entry(should_descend)
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(&base).unwrap_or(entry.path());
        if matcher.is_match(relative) || matcher.is_match(entry.path()) {
            paths.push(relative.display().to_string());
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
        partial: false,
    })
}

fn should_descend(entry: &DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();
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
mod tests {
    use super::*;

    #[tokio::test]
    async fn glob_returns_sorted_paginated_search_results() {
        let dir = tempfile::tempdir().expect("temp dir should create");
        tokio::fs::write(dir.path().join("b.rs"), "")
            .await
            .expect("write should succeed");
        tokio::fs::write(dir.path().join("a.rs"), "")
            .await
            .expect("write should succeed");
        let context = crate::ToolContext::new("session", "user").with_project_root(dir.path());

        let result = run(
            &GlobFilesRequest {
                pattern: "*.rs".to_string(),
                path: None,
                limit: Some(1),
                offset: Some(1),
            },
            &context,
        )
        .await
        .expect("glob should succeed");

        assert_eq!(result.total_matches, 2);
        assert_eq!(result.matches[0].path, "b.rs");
        assert!(!result.more_available);
    }
}
