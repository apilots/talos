use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use talos_error::{TalosError, TalosResult};
use talos_tools_sandbox::ToolFileSystemHandle;
use talos_tools_sandbox::{AbsolutePathBuf, FileSystemOperation, RequestedFileSystemAccess};

use crate::runtimes::{map_sandbox_error, resolve_base_dir};
use crate::state::{sha256_hex, ReadFileStateSource};
use crate::ToolContext;

const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 2_000;
const MAX_LINE_LENGTH: usize = 500;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const TAB_WIDTH: usize = 4;
mod stream;

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
    #[serde(default)]
    pub byte_offset: Option<u64>,
    #[serde(default)]
    pub max_bytes: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileResult {
    pub path: String,
    pub offset: usize,
    pub limit: usize,
    pub total_lines: Option<usize>,
    pub next_byte_offset: Option<u64>,
    pub content: String,
    pub truncated: bool,
    pub unchanged: bool,
}

pub async fn read_text_file(
    request: &ReadFileRequest,
    context: &ToolContext,
) -> TalosResult<(PathBuf, String)> {
    let path = resolve_requested_path(request, context)?;
    let file_system = context.sandbox_file_system()?;
    ensure_readable_regular_file(&path, &file_system).await?;
    let bytes = file_system.read(&path).await.map_err(map_sandbox_error)?;
    if !looks_like_text(&bytes) {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "The requested file is not a supported text file.",
        )
        .context(path.display()));
    }
    let content = String::from_utf8(bytes).map_err(|error| {
        TalosError::from_source(
            talos_error::ErrorCode::ToolExecutionFailed,
            "The requested file is not valid UTF-8 text.",
            error.utf8_error(),
        )
    })?;
    Ok((path, content))
}

pub async fn run(request: &ReadFileRequest, context: &ToolContext) -> TalosResult<ReadFileResult> {
    if request.offset == 0 {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "offset must be a 1-indexed line number".to_string(),
        ));
    }

    if request.limit == 0 {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "limit must be greater than zero".to_string(),
        ));
    }

    let path = resolve_requested_path(request, context)?;
    let size = context
        .sandbox_file_system()?
        .metadata(&path)
        .await
        .map_err(map_sandbox_error)?
        .len;
    if size > MAX_FILE_BYTES || request.byte_offset.is_some() || request.max_bytes.is_some() {
        return stream::run(request, &path, context).await;
    }
    let (path, content) = read_text_file(request, context).await?;
    let limit = request.limit.min(MAX_LIMIT);
    let lines = content.lines().count();
    let truncated = request.offset.saturating_sub(1) + limit < lines;
    let same_range =
        context
            .read_file_state()
            .record_read_range(path.clone(), request.offset, limit);
    let unchanged = same_range && is_unchanged_read(&path, &content, context);
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
        total_lines: Some(lines),
        next_byte_offset: None,
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
        .ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                "path is required".to_string(),
            )
        })?;
    resolve_tool_path(raw_path, context)
}

pub fn requested_file_system_access(
    request: &ReadFileRequest,
    context: &ToolContext,
) -> TalosResult<Vec<RequestedFileSystemAccess>> {
    let path = resolve_requested_path(request, context)?;
    let path = normalize_existing_tool_path(&path)?;
    Ok(vec![RequestedFileSystemAccess::new(
        FileSystemOperation::Read,
        vec![path],
    )
    .map_err(|error| {
        TalosError::from_source(
            talos_error::ErrorCode::ToolExecutionFailed,
            "The requested filesystem access is invalid.",
            error,
        )
    })?])
}

pub fn normalize_existing_tool_path(path: &Path) -> TalosResult<AbsolutePathBuf> {
    let path = std::fs::canonicalize(path).map_err(|error| {
        let (code, message) = match error.kind() {
            std::io::ErrorKind::NotFound => (
                talos_error::ErrorCode::PathNotFound,
                "The requested file path was not found.",
            ),
            std::io::ErrorKind::PermissionDenied => (
                talos_error::ErrorCode::PermissionDenied,
                "Permission was denied while resolving the requested file path.",
            ),
            _ => (
                talos_error::ErrorCode::InternalError,
                "The requested file path could not be resolved; technical details were withheld.",
            ),
        };
        TalosError::from_source(code, message, error).context(path.display())
    })?;
    AbsolutePathBuf::new(path).map_err(|error| {
        TalosError::internal_with_source("The resolved file path is invalid.", error)
    })
}

pub fn normalize_write_scope(path: &Path) -> TalosResult<AbsolutePathBuf> {
    let mut existing = path.parent().ok_or_else(|| {
        TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "The requested filesystem path has no parent directory.",
        )
        .context(path.display())
    })?;
    while !existing.exists() {
        existing = existing.parent().ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                "The requested filesystem path has no existing ancestor.",
            )
            .context(path.display())
        })?;
    }
    normalize_existing_tool_path(existing)
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

async fn ensure_readable_regular_file(
    path: &Path,
    file_system: &ToolFileSystemHandle,
) -> TalosResult<()> {
    let metadata = file_system
        .metadata(path)
        .await
        .map_err(map_sandbox_error)?;
    if metadata.is_directory {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "The requested path is a directory; use Glob to inspect directory contents.",
        )
        .context(path.display()));
    }
    if !metadata.is_file {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "The requested path is not a regular file and cannot be read by this tool.",
        )
        .context(path.display()));
    }
    if metadata.len > MAX_FILE_BYTES {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            format!(
                "The requested file is too large to read inline ({} bytes, max {} bytes).",
                metadata.len, MAX_FILE_BYTES
            ),
        )
        .context(path.display()));
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
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "The requested offset exceeds the file length.",
        )
        .context(path.display()));
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
#[path = "fs_read_tests.rs"]
mod tests;
