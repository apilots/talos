use serde::{Deserialize, Serialize};
use std::path::Path;
use talos_error::{TalosError, TalosResult};

use crate::runtimes::fs_read::{normalize_write_scope, resolve_tool_path};
use crate::runtimes::map_sandbox_error;
use crate::state::sha256_hex;
use crate::ToolContext;
use talos_tools_sandbox::{FileSystemOperation, RequestedFileSystemAccess};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFileRequest {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriteFileChangeKind {
    Create,
    Update,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFileResult {
    pub path: String,
    pub kind: WriteFileChangeKind,
    pub original_file: Option<String>,
    pub structured_patch: String,
}

pub fn requested_file_system_access(
    request: &WriteFileRequest,
    context: &ToolContext,
) -> TalosResult<Vec<RequestedFileSystemAccess>> {
    let raw_path = request
        .path
        .as_deref()
        .or(request.file_path.as_deref())
        .ok_or_else(|| {
            TalosError::new(talos_error::ErrorCode::InvalidRequest, "path is required")
        })?;
    let path = resolve_tool_path(raw_path, context)?;
    let scope = normalize_write_scope(&path)?;
    Ok(vec![RequestedFileSystemAccess::new(
        FileSystemOperation::Write,
        vec![scope],
    )
    .map_err(|error| {
        TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            error.to_string(),
        )
    })?])
}

pub async fn run(
    request: &WriteFileRequest,
    context: &ToolContext,
) -> TalosResult<WriteFileResult> {
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
    let path = resolve_tool_path(raw_path, context)?;
    let file_system = context.sandbox_file_system()?;
    let original_file = file_system.read_to_string(&path).await.ok();

    if let Some(existing) = original_file.as_ref() {
        ensure_read_state_is_fresh(&path, existing, context)?;
    }

    if let Some(parent) = path.parent() {
        file_system
            .create_directory_all(parent)
            .await
            .map_err(map_sandbox_error)?;
    }

    file_system
        .write(&path, request.content.as_bytes())
        .await
        .map_err(map_sandbox_error)?;
    context
        .read_file_state()
        .record_mutation_content(path.clone(), &request.content);

    Ok(WriteFileResult {
        path: path.display().to_string(),
        kind: if original_file.is_some() {
            WriteFileChangeKind::Update
        } else {
            WriteFileChangeKind::Create
        },
        structured_patch: build_patch(original_file.as_deref().unwrap_or(""), &request.content),
        original_file,
    })
}

pub fn ensure_read_state_is_fresh(
    path: &Path,
    current_content: &str,
    context: &ToolContext,
) -> TalosResult<()> {
    let entry = context.read_file_state().get(path).ok_or_else(|| {
        TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            format!(
                "file `{}` must be read before write or edit",
                path.display()
            ),
        )
    })?;
    let current_hash = sha256_hex(current_content.as_bytes());
    if entry.content_hash != current_hash || entry.size_bytes != current_content.len() as u64 {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            format!(
                "file `{}` changed since last read; read it again before modifying",
                path.display()
            ),
        ));
    }
    Ok(())
}

fn build_patch(original: &str, updated: &str) -> String {
    format!("@@ original\n-{}\n@@ updated\n+{}", original, updated)
}
