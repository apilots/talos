use globset::{Glob, GlobMatcher};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use std::path::Path;
use talos_error::{TalosError, TalosResult};
use walkdir::WalkDir;

use crate::runtimes::fs_read::resolve_tool_path;
use crate::ToolContext;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 2_000;

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GrepOutputMode {
    Content,
    FilesWithMatches,
    Count,
}

impl Default for GrepOutputMode {
    fn default() -> Self {
        Self::FilesWithMatches
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepFilesRequest {
    pub pattern: String,
    #[serde(default)]
    pub include: Option<String>,
    #[serde(default)]
    pub glob: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub output_mode: GrepOutputMode,
    #[serde(default)]
    pub context: usize,
    #[serde(default)]
    pub multiline: bool,
    #[serde(default)]
    pub case_insensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepFilesResponse {
    pub content: String,
    pub matches_found: usize,
}

pub async fn run(
    request: &GrepFilesRequest,
    context: &ToolContext,
) -> TalosResult<GrepFilesResponse> {
    let pattern = request.pattern.trim();
    if pattern.is_empty() {
        return Err(TalosError::ToolError(
            "pattern must not be empty".to_string(),
        ));
    }
    if request.limit == 0 {
        return Err(TalosError::ToolError(
            "limit must be greater than zero".to_string(),
        ));
    }

    let search_path = if let Some(path) = request.path.as_deref() {
        resolve_tool_path(path, context)?
    } else {
        crate::runtimes::resolve_base_dir(context)?
    };

    let glob = request.glob.as_deref().or(request.include.as_deref());
    let matcher = build_glob_matcher(glob)?;
    let regex = RegexBuilder::new(pattern)
        .case_insensitive(request.case_insensitive)
        .multi_line(request.multiline)
        .build()
        .map_err(|err| TalosError::ToolError(format!("invalid grep pattern: {}", err)))?;

    let mut file_hits = Vec::new();
    let mut content_hits = Vec::new();
    let mut total_matches = 0usize;

    for entry in WalkDir::new(&search_path)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        if !matches_glob(path, matcher.as_ref()) {
            continue;
        }
        let Ok(content) = tokio::fs::read_to_string(path).await else {
            continue;
        };
        let mut local_count = 0usize;
        for (line_index, line) in content.lines().enumerate() {
            if regex.is_match(line) {
                local_count += 1;
                total_matches += 1;
                if matches!(request.output_mode, GrepOutputMode::Content) {
                    content_hits.push(format!(
                        "{}:{}:{}",
                        path.display(),
                        line_index + 1,
                        line.trim()
                    ));
                }
            }
        }
        if local_count > 0 {
            file_hits.push(path.display().to_string());
        }
    }

    let offset = request.offset.min(MAX_LIMIT);
    let limit = request.limit.min(MAX_LIMIT);
    let content = match request.output_mode {
        GrepOutputMode::FilesWithMatches => file_hits
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>()
            .join("\n"),
        GrepOutputMode::Count => total_matches.to_string(),
        GrepOutputMode::Content => content_hits
            .into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>()
            .join("\n"),
    };

    Ok(GrepFilesResponse {
        content,
        matches_found: total_matches,
    })
}

fn build_glob_matcher(glob: Option<&str>) -> TalosResult<Option<GlobMatcher>> {
    let Some(glob) = glob.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let matcher = Glob::new(glob)
        .map_err(|err| TalosError::ToolError(format!("invalid glob: {}", err)))?
        .compile_matcher();
    Ok(Some(matcher))
}

fn matches_glob(path: &Path, matcher: Option<&GlobMatcher>) -> bool {
    matcher.map(|item| item.is_match(path)).unwrap_or(true)
}
