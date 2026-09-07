use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    let all_matches = parse_rg_json(&rg_output.stdout)?;
    let total_matches = all_matches.len();
    let offset = request.offset.min(total_matches);
    let selected = all_matches
        .iter()
        .skip(offset)
        .take(limit)
        .cloned()
        .collect::<Vec<_>>();
    let more_available = offset + selected.len() < total_matches;
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
    stdout: String,
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
        args.extend(["-C".to_string(), context_lines.to_string()]);
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
    let mut stdout = child.take_stdout().ok_or_else(|| {
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

    let stdout_task = tokio::spawn(async move {
        let mut output = String::new();
        let _ = stdout.read_to_string(&mut output).await;
        output
    });
    let stderr_task = tokio::spawn(async move {
        let mut output = String::new();
        let _ = stderr.read_to_string(&mut output).await;
        output
    });

    let timeout = Duration::from_millis(request.timeout_ms.max(1));
    let timed_out = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(status) => {
            let status = status.map_err(|err| {
                TalosError::new(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    format!("failed to wait for rg: {err}"),
                )
            })?;
            if !status.success() && status.code() != Some(1) {
                let stderr = stderr_task.await.unwrap_or_default();
                return Err(TalosError::new(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    format!("rg failed with status {status}: {}", stderr.trim()),
                ));
            }
            false
        }
        Err(_) => {
            let _ = child.kill().await;
            true
        }
    };

    let stdout = stdout_task.await.unwrap_or_default();
    if timed_out {
        let _ = stderr_task.await;
    }
    Ok(RgOutput { stdout, timed_out })
}

fn parse_rg_json(stdout: &str) -> TalosResult<Vec<TalosToolSearchMatch>> {
    let mut matches: Vec<TalosToolSearchMatch> = Vec::new();
    let mut pending_before = Vec::new();
    let mut last_match_index: Option<usize> = None;

    for raw_line in stdout.lines() {
        let event: Value = serde_json::from_str(raw_line).map_err(|err| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                format!("invalid rg json output: {err}"),
            )
        })?;
        let Some(event_type) = event.get("type").and_then(Value::as_str) else {
            continue;
        };
        let data = event.get("data").unwrap_or(&Value::Null);
        match event_type {
            "context" => {
                let context_line = line_text(data);
                if let Some(index) = last_match_index {
                    matches[index].after.push(context_line);
                } else {
                    pending_before.push(context_line);
                }
            }
            "match" => {
                let path = data
                    .get("path")
                    .and_then(|path| path.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let line = data
                    .get("line_number")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .unwrap_or(0);
                let column = data
                    .get("submatches")
                    .and_then(Value::as_array)
                    .and_then(|items| items.first())
                    .and_then(|item| item.get("start"))
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value + 1).ok());
                matches.push(TalosToolSearchMatch {
                    path,
                    line,
                    column,
                    text: line_text(data),
                    before: std::mem::take(&mut pending_before),
                    after: Vec::new(),
                });
                last_match_index = Some(matches.len() - 1);
            }
            "begin" | "end" | "summary" => {}
            _ => {}
        }
    }

    Ok(matches)
}

fn line_text(data: &Value) -> String {
    data.get("lines")
        .and_then(|lines| lines.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim_end_matches(['\r', '\n'])
        .to_string()
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
