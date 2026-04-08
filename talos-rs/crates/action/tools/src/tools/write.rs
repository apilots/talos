use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use talos_error::TalosResult;

use crate::{decode_tool_input, Tool, ToolContext, ToolOutput};

/// Tool for writing content to a file
pub struct WriteTool;

#[derive(Debug, Deserialize)]
struct WriteToolInput {
    path: String,
    content: String,
}

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Write content to a file, creating it if it does not exist or overwriting it entirely if it does.\n\nUsage:\n- path must be an absolute path\n- content replaces the entire file; partial updates should use edit instead\n- To preserve existing content, read the file first and incorporate the existing content into the new write\n\nIMPORTANT: This tool overwrites the entire file. Use edit for targeted changes to existing files."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["path", "content"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the file to create or overwrite."
                },
                "content": {
                    "type": "string",
                    "description": "Full file contents to write. This replaces the entire file."
                }
            }
        }))
    }

    async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
        let input: WriteToolInput = match decode_tool_input(input, self.name()) {
            Ok(input) => input,
            Err(err) => {
                let error = if err.to_string().contains("missing field `path`") {
                    "Input must contain 'path' field".to_string()
                } else if err.to_string().contains("missing field `content`") {
                    "Input must contain 'content' field".to_string()
                } else {
                    err.to_string()
                };
                return Ok(ToolOutput {
                    success: false,
                    output: String::new(),
                    error: Some(error),
                });
            }
        };

        match fs::write(&input.path, &input.content) {
            Ok(_) => Ok(ToolOutput {
                success: true,
                output: format!(
                    "Successfully wrote {} characters to {}",
                    input.content.len(),
                    input.path
                ),
                error: None,
            }),
            Err(err) => Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some(format!("Failed to write to file '{}': {}", input.path, err)),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_write_tool_success() {
        // Create a temporary file
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();
        std::fs::remove_file(&path).unwrap(); // Remove the temp file to write to it

        let tool = WriteTool;
        let input = serde_json::json!({
            "path": path,
            "content": "Hello, World!"
        });
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(result.success);
        assert!(result.output.contains("Successfully wrote"));

        // Verify the content was written
        let written_content = fs::read_to_string(&path).unwrap();
        assert_eq!(written_content, "Hello, World!");
    }

    #[tokio::test]
    async fn test_write_tool_missing_path() {
        let tool = WriteTool;
        let input = serde_json::json!({
            "content": "Hello, World!"
        });
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("Input must contain 'path' field"));
    }

    #[tokio::test]
    async fn test_write_tool_missing_content() {
        let tool = WriteTool;
        let input = serde_json::json!({
            "path": "/tmp/test.txt"
        });
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("Input must contain 'content' field"));
    }
}
