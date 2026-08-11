use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use talos_error::{TalosError, TalosResult};
use talos_types::talos::TalosToolSearchMatch;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::runtimes::fs_read::resolve_tool_path;
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
        return Err(TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            "pattern must not be empty".to_string(),
        ));
    }
    if request.limit == 0 {
        return Err(TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            "limit must be greater than zero".to_string(),
        ));
    }

    let search_path = if let Some(path) = request.path.as_deref() {
        resolve_tool_path(path, context)?
    } else {
        crate::runtimes::resolve_base_dir(context)?
    };
    let limit = request.head_limit.unwrap_or(request.limit).min(MAX_LIMIT);
    let context_lines = request
        .context
        .max(request.before_context.unwrap_or(0))
        .max(request.after_context.unwrap_or(0));
    let rg_output = run_rg(request, pattern, &search_path, context_lines).await?;
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
    search_path: &PathBuf,
    context_lines: usize,
) -> TalosResult<RgOutput> {
    let mut command = Command::new("rg");
    command
        .arg("--json")
        .arg("--line-number")
        .arg("--column")
        .arg("--with-filename")
        .arg("--color")
        .arg("never")
        .arg("--no-heading")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    if context_lines > 0 {
        command.arg("-C").arg(context_lines.to_string());
    }
    if request.multiline {
        command.arg("--multiline");
    }
    if request.case_insensitive {
        command.arg("--ignore-case");
    }
    if let Some(glob) = request.glob.as_deref().or(request.include.as_deref()) {
        if !glob.trim().is_empty() {
            command.arg("--glob").arg(glob);
        }
    }
    if let Some(kind) = request.r#type.as_deref() {
        if !kind.trim().is_empty() {
            command.arg("--type").arg(kind);
        }
    }
    command.arg(pattern).arg(search_path);

    let mut child = command.spawn().map_err(|err| {
        TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            format!("failed to execute rg: {err}"),
        )
    })?;
    let mut stdout = child.stdout.take().ok_or_else(|| {
        TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            "failed to capture rg stdout".to_string(),
        )
    })?;
    let mut stderr = child.stderr.take().ok_or_else(|| {
        TalosError::public(
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
                TalosError::public(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    format!("failed to wait for rg: {err}"),
                )
            })?;
            if !status.success() && status.code() != Some(1) {
                let stderr = stderr_task.await.unwrap_or_default();
                return Err(TalosError::public(
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
            TalosError::public(
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
