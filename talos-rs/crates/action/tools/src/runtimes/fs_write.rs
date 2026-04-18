use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use talos_error::{TalosError, TalosResult};
use tokio::fs;

use crate::runtimes::fs_read::resolve_tool_path;
use crate::state::sha256_hex;
use crate::ToolContext;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFileRequest {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFileResult {
    pub path: String,
    pub kind: String,
    pub original_file: Option<String>,
    pub structured_patch: String,
}

pub async fn run(
    request: &WriteFileRequest,
    context: &ToolContext,
) -> TalosResult<WriteFileResult> {
    let raw_path = request
        .path
        .as_deref()
        .or(request.file_path.as_deref())
        .ok_or_else(|| TalosError::ToolError("path is required".to_string()))?;
    let path = resolve_tool_path(raw_path, context)?;
    let original_file = fs::read_to_string(&path).await.ok();

    if let Some(existing) = original_file.as_ref() {
        ensure_read_state_is_fresh(&path, existing, context)?;
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.map_err(|err| {
            TalosError::ToolError(format!("failed to create parent dir: {}", err))
        })?;
    }

    fs::write(&path, &request.content)
        .await
        .map_err(|err| TalosError::ToolError(format!("failed to write file: {}", err)))?;
    context
        .read_file_state()
        .record_content(path.clone(), &request.content);

    Ok(WriteFileResult {
        path: path.display().to_string(),
        kind: if original_file.is_some() {
            "update".to_string()
        } else {
            "create".to_string()
        },
        structured_patch: build_patch(original_file.as_deref().unwrap_or(""), &request.content),
        original_file,
    })
}

pub fn ensure_read_state_is_fresh(
    path: &PathBuf,
    current_content: &str,
    context: &ToolContext,
) -> TalosResult<()> {
    let entry = context.read_file_state().get(path).ok_or_else(|| {
        TalosError::ToolError(format!(
            "file `{}` must be read before write or edit",
            path.display()
        ))
    })?;
    let current_hash = sha256_hex(current_content.as_bytes());
    if entry.content_hash != current_hash || entry.size_bytes != current_content.len() as u64 {
        return Err(TalosError::ToolError(format!(
            "file `{}` changed since last read; read it again before modifying",
            path.display()
        )));
    }
    Ok(())
}

fn build_patch(original: &str, updated: &str) -> String {
    format!("@@ original\n-{}\n@@ updated\n+{}", original, updated)
}
