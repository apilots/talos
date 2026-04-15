use async_trait::async_trait;
use serde_json::Value;
use talos_error::TalosResult;

use crate::runtimes::fs_list::{run, ListDirRequest};
use crate::{Tool, ToolContext, ToolOutput};

pub struct ListDirTool;

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List a local directory with bounded depth to map repository or subsystem structure before reading files. Prefer this over shell ls or find when exploring an unfamiliar project area. Use list_dir to confirm top-level layout, then switch to grep_files or read_file once the search scope is clear. If you are unsure whether a path is a file or directory, call list_dir before read_file."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["dir_path"],
            "properties": {
                "dir_path": {
                    "type": "string",
                    "description": "Absolute path to the directory to inspect. Must start with /."
                },
                "offset": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "1-based offset into the directory entry list for pagination."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum number of directory entries to return in this call."
                },
                "depth": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum recursion depth when expanding nested directories."
                }
            }
        }))
    }

    async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
        let request: ListDirRequest = serde_json::from_value(input).map_err(|err| {
            talos_error::TalosError::ToolError(format!("invalid list_dir arguments: {}", err))
        })?;
        let output = run(&request).await?;
        Ok(ToolOutput {
            success: true,
            output,
            error: None,
        })
    }
}
