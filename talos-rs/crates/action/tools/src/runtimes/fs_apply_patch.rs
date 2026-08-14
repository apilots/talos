use serde::{Deserialize, Serialize};
use std::path::Path;
use talos_error::{TalosError, TalosResult};

use crate::runtimes::fs_read::{
    normalize_existing_tool_path, normalize_write_scope, resolve_tool_path,
};
use crate::runtimes::fs_write::ensure_read_state_is_fresh;
use crate::runtimes::map_sandbox_file_error;
use crate::ToolContext;
use talos_tools_sandbox::{FileSystemOperation, RequestedFileSystemAccess};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyPatchRequest {
    pub patch: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApplyPatchOperationKind {
    Add,
    Update,
    Delete,
    Move,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyPatchFileChange {
    pub path: String,
    #[serde(default)]
    pub old_path: Option<String>,
    pub kind: ApplyPatchOperationKind,
    pub original_file: String,
    pub updated_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyPatchResult {
    pub changes: Vec<ApplyPatchFileChange>,
}

pub fn requested_file_system_access(
    request: &ApplyPatchRequest,
    context: &ToolContext,
) -> TalosResult<Vec<RequestedFileSystemAccess>> {
    let parsed = parse_patch(&request.patch)?;
    let mut read_paths = Vec::new();
    let mut write_paths = Vec::new();
    for operation in parsed.operations {
        match operation {
            PatchOperation::Add { path, .. } => {
                let path = resolve_tool_path(&path, context)?;
                write_paths.push(normalize_write_scope(&path)?);
            }
            PatchOperation::Delete { path } => {
                let path = normalize_existing_tool_path(&resolve_tool_path(&path, context)?)?;
                read_paths.push(path.clone());
                write_paths.push(path);
            }
            PatchOperation::Update { path, move_to, .. } => {
                let source_path = resolve_tool_path(&path, context)?;
                let source = normalize_existing_tool_path(&source_path)?;
                read_paths.push(source.clone());
                write_paths.push(normalize_write_scope(&source_path)?);
                if let Some(target) = move_to {
                    write_paths.push(normalize_write_scope(&resolve_tool_path(
                        &target, context,
                    )?)?);
                }
            }
        }
    }
    let mut access = Vec::new();
    if !read_paths.is_empty() {
        access.push(
            RequestedFileSystemAccess::new(FileSystemOperation::Read, read_paths)
                .map_err(|error| TalosError::ToolError(error.to_string()))?,
        );
    }
    if !write_paths.is_empty() {
        access.push(
            RequestedFileSystemAccess::new(FileSystemOperation::Write, write_paths)
                .map_err(|error| TalosError::ToolError(error.to_string()))?,
        );
    }
    Ok(access)
}

#[derive(Debug, Clone)]
struct ParsedPatch {
    operations: Vec<PatchOperation>,
}

#[derive(Debug, Clone)]
enum PatchOperation {
    Add {
        path: String,
        lines: Vec<String>,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        move_to: Option<String>,
        hunks: Vec<PatchHunk>,
    },
}

#[derive(Debug, Clone)]
struct PatchHunk {
    lines: Vec<PatchLine>,
}

#[derive(Debug, Clone)]
enum PatchLine {
    Context(String),
    Added(String),
    Removed(String),
}

pub async fn run(
    request: &ApplyPatchRequest,
    context: &ToolContext,
) -> TalosResult<ApplyPatchResult> {
    let parsed = parse_patch(&request.patch)?;
    let file_system = context.sandbox_file_system()?;
    let mut changes = Vec::new();

    for operation in parsed.operations {
        match operation {
            PatchOperation::Add { path, lines } => {
                let resolved = resolve_tool_path(&path, context)?;
                if file_system
                    .try_exists(&resolved)
                    .await
                    .map_err(map_sandbox_file_error)?
                {
                    return Err(TalosError::ToolError(format!(
                        "cannot add `{}` because it already exists",
                        resolved.display()
                    )));
                }
                if let Some(parent) = resolved.parent() {
                    file_system
                        .create_directory_all(parent)
                        .await
                        .map_err(map_sandbox_file_error)?;
                }
                let updated_file = lines.join("\n");
                file_system
                    .write(&resolved, updated_file.as_bytes())
                    .await
                    .map_err(map_sandbox_file_error)?;
                context
                    .read_file_state()
                    .record_mutation_content(resolved.clone(), &updated_file);
                changes.push(ApplyPatchFileChange {
                    path: resolved.display().to_string(),
                    old_path: None,
                    kind: ApplyPatchOperationKind::Add,
                    original_file: String::new(),
                    updated_file,
                });
            }
            PatchOperation::Delete { path } => {
                let resolved = resolve_tool_path(&path, context)?;
                let original_file = file_system
                    .read_to_string(&resolved)
                    .await
                    .map_err(map_sandbox_file_error)?;
                ensure_read_state_is_fresh(&resolved, &original_file, context)?;
                file_system
                    .remove_file(&resolved)
                    .await
                    .map_err(map_sandbox_file_error)?;
                changes.push(ApplyPatchFileChange {
                    path: resolved.display().to_string(),
                    old_path: None,
                    kind: ApplyPatchOperationKind::Delete,
                    original_file,
                    updated_file: String::new(),
                });
            }
            PatchOperation::Update {
                path,
                move_to,
                hunks,
            } => {
                let source = resolve_tool_path(&path, context)?;
                let original_file = file_system
                    .read_to_string(&source)
                    .await
                    .map_err(map_sandbox_file_error)?;
                ensure_read_state_is_fresh(&source, &original_file, context)?;
                let updated_file = apply_hunks(&source, &original_file, &hunks)?;
                let target = if let Some(raw_target) = move_to.as_deref() {
                    resolve_tool_path(raw_target, context)?
                } else {
                    source.clone()
                };
                if target != source
                    && file_system
                        .try_exists(&target)
                        .await
                        .map_err(map_sandbox_file_error)?
                {
                    return Err(TalosError::ToolError(format!(
                        "cannot move `{}` to `{}` because the target exists",
                        source.display(),
                        target.display()
                    )));
                }
                if let Some(parent) = target.parent() {
                    file_system
                        .create_directory_all(parent)
                        .await
                        .map_err(map_sandbox_file_error)?;
                }
                if target != source {
                    file_system
                        .rename(&source, &target)
                        .await
                        .map_err(map_sandbox_file_error)?;
                }
                file_system
                    .write(&target, updated_file.as_bytes())
                    .await
                    .map_err(map_sandbox_file_error)?;
                context
                    .read_file_state()
                    .record_mutation_content(target.clone(), &updated_file);
                changes.push(ApplyPatchFileChange {
                    path: target.display().to_string(),
                    old_path: if target != source {
                        Some(source.display().to_string())
                    } else {
                        None
                    },
                    kind: if target != source {
                        ApplyPatchOperationKind::Move
                    } else {
                        ApplyPatchOperationKind::Update
                    },
                    original_file,
                    updated_file,
                });
            }
        }
    }

    Ok(ApplyPatchResult { changes })
}

fn parse_patch(patch: &str) -> TalosResult<ParsedPatch> {
    let lines = patch
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect::<Vec<_>>();
    let lines = trim_trailing_empty_lines(lines);
    if lines.first().map(String::as_str) != Some("*** Begin Patch") {
        return Err(TalosError::ToolError(
            "patch must start with `*** Begin Patch`".to_string(),
        ));
    }
    if lines.last().map(String::as_str) != Some("*** End Patch") {
        return Err(TalosError::ToolError(
            "patch must end with `*** End Patch`".to_string(),
        ));
    }

    let mut index = 1usize;
    let mut operations = Vec::new();
    while index + 1 < lines.len() {
        let line = &lines[index];
        if let Some(path) = line.strip_prefix("*** Add File: ") {
            index += 1;
            let mut add_lines = Vec::new();
            while index < lines.len() && !lines[index].starts_with("*** ") {
                let line = &lines[index];
                let Some(added) = line.strip_prefix('+') else {
                    return Err(patch_syntax_error(
                        index + 1,
                        "add file lines must start with `+`",
                    ));
                };
                add_lines.push(added.to_string());
                index += 1;
            }
            operations.push(PatchOperation::Add {
                path: path.to_string(),
                lines: add_lines,
            });
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Delete File: ") {
            operations.push(PatchOperation::Delete {
                path: path.to_string(),
            });
            index += 1;
            continue;
        }
        if let Some(path) = line.strip_prefix("*** Update File: ") {
            index += 1;
            let mut move_to = None;
            if index < lines.len() {
                if let Some(target) = lines[index].strip_prefix("*** Move to: ") {
                    move_to = Some(target.to_string());
                    index += 1;
                }
            }
            let mut hunks = Vec::new();
            while index < lines.len() && !lines[index].starts_with("*** ") {
                if !lines[index].starts_with("@@") {
                    return Err(patch_syntax_error(
                        index + 1,
                        "update hunks must start with `@@`",
                    ));
                }
                index += 1;
                let mut hunk_lines = Vec::new();
                while index < lines.len()
                    && !lines[index].starts_with("@@")
                    && !lines[index].starts_with("*** ")
                {
                    let line = &lines[index];
                    let Some(prefix) = line.chars().next() else {
                        return Err(patch_syntax_error(index + 1, "empty hunk line is invalid"));
                    };
                    let text = line.get(1..).unwrap_or_default().to_string();
                    match prefix {
                        ' ' => hunk_lines.push(PatchLine::Context(text)),
                        '+' => hunk_lines.push(PatchLine::Added(text)),
                        '-' => hunk_lines.push(PatchLine::Removed(text)),
                        _ => {
                            return Err(patch_syntax_error(
                                index + 1,
                                "hunk lines must start with space, `+`, or `-`",
                            ));
                        }
                    }
                    index += 1;
                }
                hunks.push(PatchHunk { lines: hunk_lines });
            }
            if hunks.is_empty() && move_to.is_none() {
                return Err(patch_syntax_error(
                    index + 1,
                    "update file requires a hunk or move",
                ));
            }
            operations.push(PatchOperation::Update {
                path: path.to_string(),
                move_to,
                hunks,
            });
            continue;
        }
        return Err(patch_syntax_error(
            index + 1,
            "expected add, update, delete, or end patch marker",
        ));
    }

    if operations.is_empty() {
        return Err(TalosError::ToolError(
            "patch must include at least one file operation".to_string(),
        ));
    }
    Ok(ParsedPatch { operations })
}

fn trim_trailing_empty_lines(mut lines: Vec<String>) -> Vec<String> {
    while lines.last().map(|line| line.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    lines
}

fn apply_hunks(path: &Path, original: &str, hunks: &[PatchHunk]) -> TalosResult<String> {
    let mut updated = original.to_string();
    for (hunk_index, hunk) in hunks.iter().enumerate() {
        let old = hunk_old_text(hunk);
        let new = hunk_new_text(hunk);
        if old.is_empty() {
            return Err(TalosError::ToolError(format!(
                "patch hunk {} for `{}` has no removable or context text",
                hunk_index + 1,
                path.display()
            )));
        }
        let matches = updated.matches(&old).count();
        if matches == 0 {
            return Err(TalosError::ToolError(format!(
                "patch hunk {} did not match `{}`",
                hunk_index + 1,
                path.display()
            )));
        }
        if matches > 1 {
            return Err(TalosError::ToolError(format!(
                "patch hunk {} matched {} locations in `{}`; add more context",
                hunk_index + 1,
                matches,
                path.display()
            )));
        }
        updated = updated.replacen(&old, &new, 1);
    }
    Ok(updated)
}

fn hunk_old_text(hunk: &PatchHunk) -> String {
    hunk.lines
        .iter()
        .filter_map(|line| match line {
            PatchLine::Context(text) | PatchLine::Removed(text) => Some(text.as_str()),
            PatchLine::Added(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn hunk_new_text(hunk: &PatchHunk) -> String {
    hunk.lines
        .iter()
        .filter_map(|line| match line {
            PatchLine::Context(text) | PatchLine::Added(text) => Some(text.as_str()),
            PatchLine::Removed(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn patch_syntax_error(line: usize, message: &str) -> TalosError {
    TalosError::ToolError(format!("patch syntax error at line {line}: {message}"))
}

#[cfg(test)]
#[path = "fs_apply_patch_tests.rs"]
mod tests;
