use async_trait::async_trait;
use serde_json::Value;
use std::fs;
use talos_error::TalosResult;

use crate::{Tool, ToolContext, ToolOutput};

/// Tool for editing file content (find and replace)
pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Edits a file by finding and replacing text. Input should contain 'path', 'find', and 'replace' fields."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["path", "find", "replace"],
            "properties": {
                "path": {"type": "string"},
                "find": {"type": "string"},
                "replace": {"type": "string"}
            }
        }))
    }

    async fn execute(&self, input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
        // Extract path, find, and replace from input
        let path = if let Some(path_val) = input.get("path") {
            path_val.as_str().unwrap_or("")
        } else {
            return Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some("Input must contain 'path' field".to_string()),
            });
        };

        let find = if let Some(find_val) = input.get("find") {
            find_val.as_str().unwrap_or("")
        } else {
            return Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some("Input must contain 'find' field".to_string()),
            });
        };

        let replace = if let Some(replace_val) = input.get("replace") {
            replace_val.as_str().unwrap_or("")
        } else {
            return Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some("Input must contain 'replace' field".to_string()),
            });
        };

        // Read the original content
        let original_content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(err) => {
                return Ok(ToolOutput {
                    success: false,
                    output: "".to_string(),
                    error: Some(format!("Failed to read file '{}': {}", path, err)),
                });
            }
        };

        // Perform the replacement
        let new_content = original_content.replace(find, replace);

        // If no replacements were made, mention this in the output
        let replacements = original_content.matches(find).count();

        if new_content == original_content {
            return Ok(ToolOutput {
                success: true,
                output: format!("No occurrences of '{}' found in {}", find, path),
                error: None,
            });
        }

        // Write the new content back to the file
        match fs::write(path, &new_content) {
            Ok(_) => Ok(ToolOutput {
                success: true,
                output: format!(
                    "Replaced {} occurrence(s) of '{}' with '{}' in {}",
                    replacements, find, replace, path
                ),
                error: None,
            }),
            Err(err) => Ok(ToolOutput {
                success: false,
                output: "".to_string(),
                error: Some(format!("Failed to write to file '{}': {}", path, err)),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_edit_tool_success() {
        // Create a temporary file with initial content
        let mut temp_file = NamedTempFile::new().unwrap();
        let initial_content = "Hello world, this is a test.";
        temp_file.write_all(initial_content.as_bytes()).unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();

        let tool = EditTool;
        let input = serde_json::json!({
            "path": path,
            "find": "world",
            "replace": "Rust"
        });
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(result.success);
        assert!(result.output.contains("Replaced 1 occurrence"));

        // Verify the content was changed
        let new_content = fs::read_to_string(&path).unwrap();
        assert!(new_content.contains("Hello Rust"));
    }

    #[tokio::test]
    async fn test_edit_tool_multiple_occurrences() {
        // Create a temporary file with initial content
        let mut temp_file = NamedTempFile::new().unwrap();
        let initial_content = "Hello world, world is great, world!";
        temp_file.write_all(initial_content.as_bytes()).unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();

        let tool = EditTool;
        let input = serde_json::json!({
            "path": path,
            "find": "world",
            "replace": "Rust"
        });
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(result.success);
        assert!(result.output.contains("Replaced 3 occurrence"));

        // Verify the content was changed
        let new_content = fs::read_to_string(&path).unwrap();
        assert_eq!(new_content, "Hello Rust, Rust is great, Rust!");
    }

    #[tokio::test]
    async fn test_edit_tool_no_matches() {
        // Create a temporary file with initial content
        let mut temp_file = NamedTempFile::new().unwrap();
        let initial_content = "Hello world, this is a test.";
        temp_file.write_all(initial_content.as_bytes()).unwrap();
        let path = temp_file.path().to_str().unwrap().to_string();

        let tool = EditTool;
        let input = serde_json::json!({
            "path": path,
            "find": "python",
            "replace": "Rust"
        });
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(result.success);
        assert!(result.output.contains("No occurrences"));

        // Verify the content was unchanged
        let new_content = fs::read_to_string(&path).unwrap();
        assert_eq!(new_content, initial_content);
    }

    #[tokio::test]
    async fn test_edit_tool_missing_fields() {
        let tool = EditTool;
        let input = serde_json::json!({
            "path": "/tmp/test.txt",
            "find": "test"
        });
        let result = tool.execute(input, ToolContext::default()).await.unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("Input must contain 'replace' field"));
    }
}
