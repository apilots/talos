use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};

use crate::runtimes::fs_read::{normalize_existing_tool_path, resolve_tool_path};
use crate::runtimes::fs_write::ensure_read_state_is_fresh;
use crate::runtimes::map_sandbox_error;
use crate::ToolContext;
use talos_tools_sandbox::{FileSystemOperation, RequestedFileSystemAccess};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditFileRequest {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub find: Option<String>,
    #[serde(default)]
    pub old_string: Option<String>,
    #[serde(default)]
    pub replace: Option<String>,
    #[serde(default)]
    pub new_string: Option<String>,
    #[serde(default)]
    pub replace_all: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditFileResult {
    pub path: String,
    pub replacements: usize,
    pub original_file: String,
    pub updated_file: String,
    pub structured_patch: String,
}

pub fn requested_file_system_access(
    request: &EditFileRequest,
    context: &ToolContext,
) -> TalosResult<Vec<RequestedFileSystemAccess>> {
    let raw_path = request
        .path
        .as_deref()
        .or(request.file_path.as_deref())
        .ok_or_else(|| {
            TalosError::new(talos_error::ErrorCode::InvalidRequest, "path is required")
        })?;
    let path = normalize_existing_tool_path(&resolve_tool_path(raw_path, context)?)?;
    Ok(vec![RequestedFileSystemAccess::new(
        FileSystemOperation::Write,
        vec![path],
    )
    .map_err(|error| {
        TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            error.to_string(),
        )
    })?])
}

pub async fn run(request: &EditFileRequest, context: &ToolContext) -> TalosResult<EditFileResult> {
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
    let old_string = request
        .find
        .as_deref()
        .or(request.old_string.as_deref())
        .ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                "find/old_string is required".to_string(),
            )
        })?;
    let new_string = request
        .replace
        .as_deref()
        .or(request.new_string.as_deref())
        .ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::ToolExecutionFailed,
                "replace/new_string is required".to_string(),
            )
        })?;

    if old_string == new_string {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            "old_string and new_string must differ".to_string(),
        ));
    }

    let path = resolve_tool_path(raw_path, context)?;
    let file_system = context.sandbox_file_system()?;
    let original_file = file_system
        .read_to_string(&path)
        .await
        .map_err(map_sandbox_error)?;
    ensure_read_state_is_fresh(&path, &original_file, context)?;

    let replacements = original_file.matches(old_string).count();
    if replacements == 0 {
        return Err(TalosError::new(
            talos_error::ErrorCode::ToolExecutionFailed,
            format!("target text not found in `{}`", path.display()),
        ));
    }
    if replacements > 1 && !request.replace_all {
        return Err(TalosError::new(talos_error::ErrorCode::ToolExecutionFailed, format!(
            "target text matched {} times in `{}`; set replace_all=true or provide a more specific old_string",
            replacements,
            path.display()
        )));
    }

    let updated = if request.replace_all {
        original_file.replace(old_string, new_string)
    } else {
        original_file.replacen(old_string, new_string, 1)
    };

    file_system
        .write(&path, updated.as_bytes())
        .await
        .map_err(map_sandbox_error)?;
    context
        .read_file_state()
        .record_mutation_content(path.clone(), &updated);

    Ok(EditFileResult {
        path: path.display().to_string(),
        replacements: if request.replace_all { replacements } else { 1 },
        original_file: original_file.clone(),
        updated_file: updated,
        structured_patch: format!("@@ find\n-{}\n@@ replace\n+{}", old_string, new_string),
    })
}
