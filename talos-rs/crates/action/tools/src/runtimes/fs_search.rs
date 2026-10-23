use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_tools_sandbox::{SandboxStdin, ToolProcessCommand};
use talos_types::talos::TalosToolSearchMatch;
use tokio::io::AsyncReadExt;

use crate::runtimes::fs_read::{normalize_existing_tool_path, resolve_tool_path};
use crate::runtimes::{map_sandbox_error, process_environment, resolve_executable};
use crate::ToolContext;

mod page;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 2_000;
const DEFAULT_TIMEOUT_MS: u64 = 10_000;

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum GrepOutputMode {
    Content,
    #[default]
    FilesWithMatches,
    Count,
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
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub head_limit: Option<usize>,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub output_mode: GrepOutputMode,
    #[serde(default)]
    pub context: usize,
    #[serde(default)]
    pub before_context: Option<usize>,
    #[serde(default)]
    pub after_context: Option<usize>,
    #[serde(default)]
    pub multiline: bool,
    #[serde(default)]
    pub case_insensitive: bool,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepFilesResponse {
    pub query: String,
    pub content: String,
    pub matches: Vec<TalosToolSearchMatch>,
    pub offset: usize,
    pub limit: usize,
    pub total_matches: usize,
    pub more_available: bool,
    pub partial: bool,
}

pub async fn run(
    request: &GrepFilesRequest,
    context: &ToolContext,
) -> TalosResult<GrepFilesResponse> {
    let pattern = request.pattern.trim();
    if request.context > 20
        || request.before_context.is_some_and(|lines| lines > 20)
        || request.after_context.is_some_and(|lines| lines > 20)
    {
        return Err(TalosError::new(
            talos_error::ErrorCode::InvalidRequest,
            "Search context cannot exceed 20 lines on either side.",
        ));
    }
    if pattern.is_empty() {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "pattern must not be empty".to_string(),
        ));
    }
    if request.limit == 0 {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "limit must be greater than zero".to_string(),
        ));
    }

    let search_path = if let Some(path) = request.path.as_deref() {
        normalize_existing_tool_path(&resolve_tool_path(path, context)?)?.into_path_buf()
    } else {
        crate::runtimes::resolve_base_dir(context)?
    };
    if search_path.is_file()
        && search_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == ".env" || name.starts_with(".env."))
    {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "searching dotenv files is not allowed".to_string(),
        ));
    }
    let limit = request.head_limit.unwrap_or(request.limit).min(MAX_LIMIT);
    let context_lines = request
        .context
        .max(request.before_context.unwrap_or(0))
        .max(request.after_context.unwrap_or(0));
    let rg_output = run_rg(request, pattern, &search_path, context_lines, context).await?;
    let total_matches = rg_output.page.total;
    let offset = request.offset.min(total_matches);
    let selected = rg_output.page.matches;
    let more_available =
        request.output_mode != GrepOutputMode::Count && offset + selected.len() < total_matches;
    let content = render_grep_content(request.output_mode.clone(), &selected, total_matches);

    Ok(GrepFilesResponse {
        query: pattern.to_string(),
        content,
        matches: selected,
        offset,
        limit,
        total_matches,
        more_available,
        partial: rg_output.timed_out,
    })
}

struct RgOutput {
    page: page::SearchPage,
    timed_out: bool,
}

async fn run_rg(
    request: &GrepFilesRequest,
    pattern: &str,
    search_path: &Path,
    context_lines: usize,
    context: &ToolContext,
) -> TalosResult<RgOutput> {
    let mut args = vec![
        "--json".to_string(),
        "--line-number".to_string(),
        "--column".to_string(),
        "--with-filename".to_string(),
        "--color".to_string(),
        "never".to_string(),
        "--no-heading".to_string(),
    ];

    if context_lines > 0 {
        args.extend(["-C".to_string(), context_lines.min(20).to_string()]);
    }
    if request.multiline {
        args.push("--multiline".to_string());
    }
    if request.case_insensitive {
        args.push("--ignore-case".to_string());
    }
    if let Some(glob) = request.glob.as_deref().or(request.include.as_deref()) {
        if !glob.trim().is_empty() {
            args.extend(["--glob".to_string(), glob.to_string()]);
        }
    }
    if let Some(kind) = request.r#type.as_deref() {
        if !kind.trim().is_empty() {
            args.extend(["--type".to_string(), kind.to_string()]);
        }
    }
    args.extend([
        "--glob".to_string(),
        "!.env".to_string(),
        "--glob".to_string(),
        "!.env.*".to_string(),
    ]);
    args.push("--".to_string());
    args.push(pattern.to_string());
    args.push(search_path.to_string_lossy().into_owned());
    let cwd = if search_path.is_dir() {
        search_path
    } else {
        search_path.parent().ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!(
                    "search path has no parent directory: {}",
                    search_path.display()
                ),
            )
        })?
    };

    let command = ToolProcessCommand {
        program: resolve_executable("rg")?,
        args,
        env: process_environment(),
        cwd: cwd.to_path_buf(),
        stdin: SandboxStdin::Closed,
    };
    let mut child = context
        .sandbox_permit()?
        .spawn(command)
        .await
        .map_err(map_sandbox_error)?;
    let stdout = child.take_stdout().ok_or_else(|| {
        TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "failed to capture rg stdout".to_string(),
        )
    })?;
    let mut stderr = child.take_stderr().ok_or_else(|| {
        TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "failed to capture rg stderr".to_string(),
        )
    })?;

    let budget = context
        .kernel_config()
        .map(|config| config.tool_result_max_bytes)
        .unwrap_or(10_000)
        .min(crate::output_budget::MAX_MODEL_TOOL_BYTES)
        .saturating_sub(1024)
        .min(8192);
    let query = request.clone();
    let stopped = tokio_util::sync::CancellationToken::new();
    let parser_stop = stopped.clone();
    let stdout_task =
        tokio::spawn(async move { page::collect(stdout, query, budget, parser_stop).await });
    let stderr_task = tokio::spawn(async move {
        let mut preview = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let read = stderr.read(&mut buffer).await.map_err(|error| {
                TalosError::internal_with_source(
                    "Read search diagnostics; technical details were withheld.",
                    error,
                )
            })?;
            if read == 0 {
                break;
            }
            let keep = read.min(4096_usize.saturating_sub(preview.len()));
            preview.extend_from_slice(&buffer[..keep]);
        }
        Ok::<_, TalosError>(preview)
    });
    let deadline = Duration::from_millis(request.timeout_ms.max(1));
    let status = tokio::time::timeout(deadline, child.wait()).await;
    let timed_out = status.is_err();
    if timed_out {
        stopped.cancel();
        child.kill().await.map_err(map_sandbox_error)?;
    }
    let page = stdout_task
        .await
        .map_err(|error| TalosError::internal_with_source("Search reader task failed.", error))?;
    let stderr = stderr_task.await.map_err(|error| {
        TalosError::internal_with_source("Search diagnostic task failed.", error)
    })??;
    let page = page?;
    if let Ok(status) = status {
        let status = status.map_err(map_sandbox_error)?;
        if !status.success() && status.code() != Some(1) {
            return Err(TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                "Search command failed.",
            )
            .context(String::from_utf8_lossy(&stderr)));
        }
    }
    Ok(RgOutput { page, timed_out })
}

fn render_grep_content(
    output_mode: GrepOutputMode,
    matches: &[TalosToolSearchMatch],
    total_matches: usize,
) -> String {
    match output_mode {
        GrepOutputMode::Count => total_matches.to_string(),
        GrepOutputMode::FilesWithMatches => {
            let mut paths = BTreeSet::new();
            for item in matches {
                paths.insert(item.path.clone());
            }
            paths.into_iter().collect::<Vec<_>>().join("\n")
        }
        GrepOutputMode::Content => matches
            .iter()
            .map(|item| format!("{}:{}:{}", item.path, item.line, item.text.trim()))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

#[cfg(test)]
#[path = "fs_search_tests.rs"]
mod tests;
