use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use talos_error::{TalosError, TalosResult};
use tokio::fs;

use crate::runtimes::resolve_base_dir;
use crate::ToolContext;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 2_000;
const MAX_LINE_LENGTH: usize = 500;
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

pub async fn read_text_file(
    request: &ReadFileRequest,
    context: &ToolContext,
) -> TalosResult<(PathBuf, String)> {
    let path = resolve_requested_path(request, context)?;
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

pub async fn run(request: &ReadFileRequest, context: &ToolContext) -> TalosResult<String> {
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
    context
        .read_file_state()
        .record_content(path.clone(), &content);
    render_numbered_lines(
        &path,
        &content,
        request.offset,
        request.limit.min(MAX_LIMIT),
    )
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

pub fn render_numbered_lines(
    path: &Path,
    content: &str,
    offset: usize,
    limit: usize,
) -> TalosResult<String> {
    let lines = content.lines().collect::<Vec<_>>();
    if offset > lines.len() && !lines.is_empty() {
        return Err(TalosError::ToolError(format!(
            "offset exceeds file length for `{}`",
            path.display()
        )));
    }
    let start = offset.saturating_sub(1);
    let mut rendered = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(start).take(limit) {
        rendered.push(format!("L{}: {}", index + 1, format_line(line)));
    }
    Ok(rendered.join("\n"))
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

        assert!(output.contains("L1: alpha"));
        assert!(output.contains("L2: beta"));
        assert!(context.read_file_state().contains(file.path()));
    }
}
