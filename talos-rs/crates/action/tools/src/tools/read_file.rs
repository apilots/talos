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
        "Read a specific local file from a user-provided absolute path and return numbered lines. Prefer this after list_dir or grep_files has narrowed the target file, or when the user already named an exact file to inspect. Use it before editing to verify the current contents."
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
