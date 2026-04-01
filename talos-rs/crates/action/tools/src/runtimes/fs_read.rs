use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};

const DEFAULT_LIMIT: usize = 2000;
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
    pub file_path: String,
    #[serde(default = "default_offset")]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

pub async fn run(request: &ReadFileRequest) -> TalosResult<String> {
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

    let path = std::path::PathBuf::from(&request.file_path);
    if !path.is_absolute() {
        return Err(TalosError::ToolError(
            "file_path must be an absolute path".to_string(),
        ));
    }

    let file = File::open(&path)
        .await
        .map_err(|err| TalosError::ToolError(format!("failed to read file: {}", err)))?;
    let mut reader = BufReader::new(file);
    let mut buffer = String::new();
    let mut line_number = 0usize;
    let mut collected = Vec::new();

    loop {
        buffer.clear();
        let bytes_read = reader
            .read_line(&mut buffer)
            .await
            .map_err(|err| TalosError::ToolError(format!("failed to read file: {}", err)))?;
        if bytes_read == 0 {
            break;
        }

        line_number += 1;
        if line_number < request.offset {
            continue;
        }
        if collected.len() >= request.limit {
            break;
        }

        let formatted = format_line(buffer.trim_end_matches(['\n', '\r']));
        collected.push(format!("L{}: {}", line_number, formatted));
    }

    if line_number < request.offset {
        return Err(TalosError::ToolError(
            "offset exceeds file length".to_string(),
        ));
    }

    Ok(collected.join("\n"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_read_file_runtime_reads_numbered_lines() {
        let mut file = NamedTempFile::new().expect("temp file should create");
        writeln!(file, "alpha").expect("write should succeed");
        writeln!(file, "beta").expect("write should succeed");

        let output = run(&ReadFileRequest {
            file_path: file.path().to_string_lossy().to_string(),
            offset: 1,
            limit: 2,
        })
        .await
        .expect("read should succeed");

        assert!(output.contains("L1: alpha"));
        assert!(output.contains("L2: beta"));
    }
}
