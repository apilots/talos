use std::collections::VecDeque;
use std::ffi::OsStr;
use std::fs::FileType;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use tokio::fs;

use crate::runtimes::truncate_text;

const DEFAULT_OFFSET: usize = 1;
const DEFAULT_LIMIT: usize = 25;
const DEFAULT_DEPTH: usize = 2;
const MAX_ENTRY_LENGTH: usize = 500;
const INDENTATION_SPACES: usize = 2;

fn default_offset() -> usize {
    DEFAULT_OFFSET
}

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

fn default_depth() -> usize {
    DEFAULT_DEPTH
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDirRequest {
    pub dir_path: String,
    #[serde(default = "default_offset")]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default = "default_depth")]
    pub depth: usize,
}

pub async fn run(request: &ListDirRequest) -> TalosResult<String> {
    if request.offset == 0 {
        return Err(TalosError::ToolError(
            "offset must be a 1-indexed entry number".to_string(),
        ));
    }
    if request.limit == 0 {
        return Err(TalosError::ToolError(
            "limit must be greater than zero".to_string(),
        ));
    }
    if request.depth == 0 {
        return Err(TalosError::ToolError(
            "depth must be greater than zero".to_string(),
        ));
    }

    let path = PathBuf::from(&request.dir_path);
    if !path.is_absolute() {
        return Err(TalosError::ToolError(
            "dir_path must be an absolute path".to_string(),
        ));
    }

    let entries = list_dir_slice(&path, request.offset, request.limit, request.depth).await?;
    let mut output = Vec::with_capacity(entries.len() + 1);
    output.push(format!("Absolute path: {}", path.display()));
    output.extend(entries);
    Ok(output.join("\n"))
}

async fn list_dir_slice(
    path: &Path,
    offset: usize,
    limit: usize,
    depth: usize,
) -> TalosResult<Vec<String>> {
    let mut entries = Vec::new();
    collect_entries(path, Path::new(""), depth, &mut entries).await?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    entries.sort_unstable_by(|a, b| a.name.cmp(&b.name));

    let start_index = offset - 1;
    if start_index >= entries.len() {
        return Err(TalosError::ToolError(
            "offset exceeds directory entry count".to_string(),
        ));
    }

    let remaining = entries.len() - start_index;
    let capped_limit = limit.min(remaining);
    let end_index = start_index + capped_limit;
    let selected = &entries[start_index..end_index];

    let mut formatted = selected.iter().map(format_entry_line).collect::<Vec<_>>();

    if end_index < entries.len() {
        formatted.push(format!("More than {} entries found", capped_limit));
    }

    Ok(formatted)
}

async fn collect_entries(
    dir_path: &Path,
    relative_prefix: &Path,
    depth: usize,
    entries: &mut Vec<DirEntry>,
) -> TalosResult<()> {
    let mut queue = VecDeque::new();
    queue.push_back((dir_path.to_path_buf(), relative_prefix.to_path_buf(), depth));

    while let Some((current_dir, prefix, remaining_depth)) = queue.pop_front() {
        let mut read_dir = fs::read_dir(&current_dir)
            .await
            .map_err(|err| TalosError::ToolError(format!("failed to read directory: {}", err)))?;

        let mut dir_entries = Vec::new();

        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|err| TalosError::ToolError(format!("failed to read directory: {}", err)))?
        {
            let file_type = entry.file_type().await.map_err(|err| {
                TalosError::ToolError(format!("failed to inspect entry: {}", err))
            })?;

            let file_name = entry.file_name();
            let relative_path = if prefix.as_os_str().is_empty() {
                PathBuf::from(&file_name)
            } else {
                prefix.join(&file_name)
            };

            let kind = DirEntryKind::from(&file_type);
            dir_entries.push((
                entry.path(),
                relative_path.clone(),
                kind,
                DirEntry {
                    name: format_entry_name(&relative_path),
                    display_name: format_entry_component(&file_name),
                    depth: prefix.components().count(),
                    kind,
                },
            ));
        }

        dir_entries.sort_unstable_by(|a, b| a.3.name.cmp(&b.3.name));

        for (entry_path, relative_path, kind, entry) in dir_entries {
            if kind == DirEntryKind::Directory && remaining_depth > 1 {
                queue.push_back((entry_path, relative_path, remaining_depth - 1));
            }
            entries.push(entry);
        }
    }

    Ok(())
}

fn format_entry_name(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    truncate_text(&normalized, MAX_ENTRY_LENGTH).0
}

fn format_entry_component(name: &OsStr) -> String {
    truncate_text(name.to_string_lossy().as_ref(), MAX_ENTRY_LENGTH).0
}

fn format_entry_line(entry: &DirEntry) -> String {
    let indent = " ".repeat(entry.depth * INDENTATION_SPACES);
    let mut name = entry.display_name.clone();
    match entry.kind {
        DirEntryKind::Directory => name.push('/'),
        DirEntryKind::Symlink => name.push('@'),
        DirEntryKind::Other => name.push('?'),
        DirEntryKind::File => {}
    }
    format!("{}{}", indent, name)
}

#[derive(Clone)]
struct DirEntry {
    name: String,
    display_name: String,
    depth: usize,
    kind: DirEntryKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DirEntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

impl From<&FileType> for DirEntryKind {
    fn from(file_type: &FileType) -> Self {
        if file_type.is_symlink() {
            Self::Symlink
        } else if file_type.is_dir() {
            Self::Directory
        } else if file_type.is_file() {
            Self::File
        } else {
            Self::Other
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_list_dir_runtime_lists_nested_entries() {
        let dir = tempdir().expect("temp dir should create");
        tokio::fs::create_dir_all(dir.path().join("src"))
            .await
            .expect("nested dir should create");
        tokio::fs::write(dir.path().join("src").join("main.rs"), "fn main() {}")
            .await
            .expect("file should write");

        let output = run(&ListDirRequest {
            dir_path: dir.path().to_string_lossy().to_string(),
            offset: 1,
            limit: 10,
            depth: 2,
        })
        .await
        .expect("list should succeed");

        assert!(output.contains("Absolute path:"));
        assert!(output.contains("src/"));
        assert!(output.contains("main.rs"));
    }
}
