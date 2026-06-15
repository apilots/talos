use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use tokio::fs;

use crate::runtimes::fs_read::resolve_tool_path;
use crate::runtimes::fs_write::ensure_read_state_is_fresh;
use crate::ToolContext;

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

pub async fn run(request: &EditFileRequest, context: &ToolContext) -> TalosResult<EditFileResult> {
    let raw_path = request
        .path
        .as_deref()
        .or(request.file_path.as_deref())
        .ok_or_else(|| TalosError::ToolError("path is required".to_string()))?;
    let old_string = request
        .find
        .as_deref()
        .or(request.old_string.as_deref())
        .ok_or_else(|| TalosError::ToolError("find/old_string is required".to_string()))?;
    let new_string = request
        .replace
        .as_deref()
        .or(request.new_string.as_deref())
        .ok_or_else(|| TalosError::ToolError("replace/new_string is required".to_string()))?;

    if old_string == new_string {
        return Err(TalosError::ToolError(
            "old_string and new_string must differ".to_string(),
        ));
    }

    let path = resolve_tool_path(raw_path, context)?;
    let original_file = fs::read_to_string(&path)
        .await
        .map_err(|err| TalosError::ToolError(format!("failed to read file: {}", err)))?;
    ensure_read_state_is_fresh(&path, &original_file, context)?;

    let replacements = original_file.matches(old_string).count();
    if replacements == 0 {
        return Err(TalosError::ToolError(format!(
            "target text not found in `{}`",
            path.display()
        )));
    }
    if replacements > 1 && !request.replace_all {
        return Err(TalosError::ToolError(format!(
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

    fs::write(&path, &updated)
        .await
        .map_err(|err| TalosError::ToolError(format!("failed to write file: {}", err)))?;
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
