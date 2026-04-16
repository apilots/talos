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
        "List a local directory with bounded depth to map repository or subsystem structure.\n\nUsage:\n- Use list_dir instead of shell ls or find for directory inspection.\n- Use list_dir when you need directory shape, top-level layout, or to confirm whether a path is a directory.\n- For code structure, symbol, export, route, or entrypoint analysis, prefer grep_files before list_dir unless the missing fact is directory shape.\n- After list_dir clarifies the path, switch to grep_files for content search or read_file for a specific file.\n- Do not use list_dir as a substitute for grep_files when the task is to locate symbols or code references.\n- Do not keep using list_dir after the relevant files are already known."
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
