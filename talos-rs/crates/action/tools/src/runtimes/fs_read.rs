use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use talos_error::{TalosError, TalosResult};
use tokio::fs;

use crate::runtimes::resolve_base_dir;
use crate::state::{sha256_hex, ReadFileStateSource};
use crate::ToolContext;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 2_000;
const MAX_LINE_LENGTH: usize = 500;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const TAB_WIDTH: usize = 4;

fn default_offset() -> usize {
    1
}

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileRequest {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default = "default_offset")]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub pages: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileResult {
    pub path: String,
    pub offset: usize,
    pub limit: usize,
    pub total_lines: usize,
    pub content: String,
    pub truncated: bool,
    pub unchanged: bool,
}

pub async fn read_text_file(
    request: &ReadFileRequest,
    context: &ToolContext,
) -> TalosResult<(PathBuf, String)> {
    let path = resolve_requested_path(request, context)?;
    ensure_readable_regular_file(&path).await?;
    let bytes = fs::read(&path)
        .await
        .map_err(|err| TalosError::ToolError(format!("failed to read file: {}", err)))?;
    if !looks_like_text(&bytes) {
        return Err(TalosError::ToolError(format!(
            "file `{}` is not a supported text file",
            path.display()
        )));
    }
    let content = String::from_utf8(bytes)
        .map_err(|_| TalosError::ToolError("file is not valid UTF-8 text".to_string()))?;
    Ok((path, content))
}

pub async fn run(request: &ReadFileRequest, context: &ToolContext) -> TalosResult<ReadFileResult> {
    if request.offset == 0 {
        return Err(TalosError::ToolError(
            "offset must be a 1-indexed line number".to_string(),
        ));
    }

    if request.limit == 0 {
        return Err(TalosError::ToolError(
            "limit must be greater than zero".to_string(),
        ));
    }

    let (path, content) = read_text_file(request, context).await?;
    let limit = request.limit.min(MAX_LIMIT);
    let lines = content.lines().count();
    let truncated = request.offset.saturating_sub(1) + limit < lines;
    let unchanged = is_unchanged_read(&path, &content, context);
    let selected_content = if unchanged {
        format!("File unchanged since last read: {}", path.display())
    } else {
        select_lines(&path, &content, request.offset, limit)?
    };
    context
        .read_file_state()
        .record_content(path.clone(), &content);
    Ok(ReadFileResult {
        path: path.display().to_string(),
        offset: request.offset.saturating_sub(1),
        limit,
        total_lines: lines,
        content: selected_content,
        truncated,
        unchanged,
    })
}

pub fn resolve_requested_path(
    request: &ReadFileRequest,
    context: &ToolContext,
) -> TalosResult<PathBuf> {
    let raw_path = request
        .path
        .as_deref()
        .or(request.file_path.as_deref())
        .ok_or_else(|| TalosError::ToolError("path is required".to_string()))?;
    resolve_tool_path(raw_path, context)
}

pub fn resolve_tool_path(raw_path: &str, context: &ToolContext) -> TalosResult<PathBuf> {
    let path = PathBuf::from(raw_path);
    let resolved = if path.is_absolute() {
        path
    } else {
        resolve_base_dir(context)?.join(path)
    };
    Ok(resolved)
}

async fn ensure_readable_regular_file(path: &Path) -> TalosResult<()> {
    let metadata = fs::metadata(path).await.map_err(|err| {
        TalosError::ToolError(format!(
            "failed to inspect file `{}`: {}{}",
            path.display(),
            err,
            path_hint(path)
        ))
    })?;
    if metadata.is_dir() {
        return Err(TalosError::ToolError(format!(
            "`{}` is a directory; use Glob to inspect directory contents",
            path.display()
        )));
    }
    if !metadata.is_file() {
        return Err(TalosError::ToolError(format!(
            "`{}` is not a regular file and cannot be read by this tool",
            path.display()
        )));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(TalosError::ToolError(format!(
            "`{}` is too large to read inline ({} bytes, max {} bytes)",
            path.display(),
            metadata.len(),
            MAX_FILE_BYTES
        )));
    }
    Ok(())
}

fn is_unchanged_read(path: &Path, content: &str, context: &ToolContext) -> bool {
    let Some(entry) = context.read_file_state().get(path) else {
        return false;
    };
    entry.source == ReadFileStateSource::Read
        && entry.content_hash == sha256_hex(content.as_bytes())
        && entry.size_bytes == content.len() as u64
}

fn path_hint(path: &Path) -> String {
    let Some(parent) = path.parent() else {
        return String::new();
    };
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return String::new();
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return String::new();
    };
    let suggestions = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|candidate| candidate.contains(name) || name.contains(candidate))
        .take(3)
        .collect::<Vec<_>>();
    if suggestions.is_empty() {
        String::new()
    } else {
        format!("; similar path(s): {}", suggestions.join(", "))
    }
}

pub fn render_numbered_lines(
    path: &Path,
    content: &str,
    offset: usize,
    limit: usize,
) -> TalosResult<String> {
    let selected = select_lines(path, content, offset, limit)?;
    let start = offset.saturating_sub(1);
    Ok(selected
        .lines()
        .enumerate()
        .map(|(index, line)| format!("L{}: {}", start + index + 1, format_line(line)))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn select_lines(path: &Path, content: &str, offset: usize, limit: usize) -> TalosResult<String> {
    let lines = content.lines().collect::<Vec<_>>();
    if offset > lines.len() && !lines.is_empty() {
        return Err(TalosError::ToolError(format!(
            "offset exceeds file length for `{}`",
            path.display()
        )));
    }
    let start = offset.saturating_sub(1);
    Ok(lines
        .iter()
        .skip(start)
        .take(limit)
        .map(|line| (*line).to_string())
        .collect::<Vec<_>>()
        .join("\n"))
}

fn format_line(line: &str) -> String {
    let expanded = expand_tabs(line);
    if expanded.len() <= MAX_LINE_LENGTH {
        return expanded;
    }

    let mut cutoff = MAX_LINE_LENGTH;
    while cutoff > 0 && !expanded.is_char_boundary(cutoff) {
        cutoff -= 1;
    }
    format!("{}...[truncated]", &expanded[..cutoff])
}

fn expand_tabs(line: &str) -> String {
    let mut expanded = String::with_capacity(line.len());
    let mut column = 0usize;

    for ch in line.chars() {
        if ch == '\t' {
            let spaces = TAB_WIDTH - (column % TAB_WIDTH);
            for _ in 0..spaces {
                expanded.push(' ');
                column += 1;
            }
        } else {
            expanded.push(ch);
            column += 1;
        }
    }

    expanded
}

fn looks_like_text(bytes: &[u8]) -> bool {
    !bytes.iter().take(1024).any(|byte| *byte == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_read_file_runtime_reads_numbered_lines() {
        let file = NamedTempFile::new().expect("temp file should create");
        tokio::fs::write(file.path(), "alpha\nbeta\n")
            .await
            .expect("write should succeed");

        let mut config = talos_config::AgentKernelConfig::default();
        config.workspace_dir = file
            .path()
            .parent()
            .expect("parent should exist")
            .to_string_lossy()
            .to_string();
        let context =
            crate::ToolContext::new_with_kernel_config("session", "user", Some(Arc::new(config)));

        let output = run(
            &ReadFileRequest {
                path: Some(file.path().to_string_lossy().to_string()),
                file_path: None,
                offset: 1,
                limit: 2,
                pages: None,
            },
            &context,
        )
        .await
        .expect("read should succeed");

        assert_eq!(output.content, "alpha\nbeta");
        assert_eq!(output.offset, 0);
        assert!(context.read_file_state().contains(file.path()));
    }

    #[tokio::test]
    async fn test_read_file_runtime_reports_zero_based_offset() {
        let file = NamedTempFile::new().expect("temp file should create");
        tokio::fs::write(file.path(), "alpha\nbeta\ngamma\n")
            .await
            .expect("write should succeed");
        let context = crate::ToolContext::new("session", "user");

        let output = run(
            &ReadFileRequest {
                path: Some(file.path().to_string_lossy().to_string()),
                file_path: None,
                offset: 2,
                limit: 1,
                pages: None,
            },
            &context,
        )
        .await
        .expect("read should succeed");

        assert_eq!(output.offset, 1);
        assert_eq!(output.content, "beta");
        assert!(!output.content.contains("L2:"));
    }

    #[tokio::test]
    async fn test_read_file_runtime_rejects_directories() {
        let dir = tempfile::tempdir().expect("temp dir should create");
        let context = crate::ToolContext::new("session", "user");

        let error = run(
            &ReadFileRequest {
                path: Some(dir.path().to_string_lossy().to_string()),
                file_path: None,
                offset: 1,
                limit: 2,
                pages: None,
            },
            &context,
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("directory"));
    }

    #[tokio::test]
    async fn test_read_file_runtime_returns_unchanged_stub_for_repeat_read() {
        let file = NamedTempFile::new().expect("temp file should create");
        tokio::fs::write(file.path(), "alpha\n")
            .await
            .expect("write should succeed");
        let context = crate::ToolContext::new("session", "user");
        let request = ReadFileRequest {
            path: Some(file.path().to_string_lossy().to_string()),
            file_path: None,
            offset: 1,
            limit: 2,
            pages: None,
        };

        run(&request, &context)
            .await
            .expect("first read should succeed");
        let output = run(&request, &context)
            .await
            .expect("second read should succeed");

        assert!(output.unchanged);
        assert!(output.content.contains("File unchanged"));
    }
}
