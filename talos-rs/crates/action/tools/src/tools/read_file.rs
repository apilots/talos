use async_trait::async_trait;
use serde_json::Value;
use talos_error::TalosResult;

use crate::runtimes::fs_read::{run, ReadFileRequest};
use crate::{Tool, ToolContext, ToolOutput};

pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read a specific local file and return numbered lines.\n\nUsage:\n- Use read_file after grep_files or list_dir has narrowed the target file, or when the user already named an exact file.\n- Use read_file instead of cat, head, tail, or ad hoc shell readers.\n- Use list_dir first when the path may be a directory.\n- When you still need to find the right file, use grep_files before read_file.\n- Do not read the same file again with identical arguments unless you need a different offset or limit to verify a specific detail."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["file_path"],
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to read. Must start with /."
                },
                "offset": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "1-based line number to start reading from. Omit to read from the beginning."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum number of lines to return. Omit to use the default read limit."
                }
            }
        }))
    }

    async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
        let request: ReadFileRequest = serde_json::from_value(input).map_err(|err| {
            talos_error::TalosError::ToolError(format!("invalid read_file arguments: {}", err))
        })?;
        let output = run(&request).await?;
        Ok(ToolOutput {
            success: true,
            output,
            error: None,
        })
    }
}
