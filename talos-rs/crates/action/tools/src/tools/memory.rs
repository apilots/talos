use async_trait::async_trait;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use talos_memory::{Memory, MemoryGetResult, MemorySearchQuery, MemorySearchResponse, MemoryStore};

use crate::{Tool, ToolContext, ToolOutput};

fn resolve_workspace_dir(context: &ToolContext) -> Option<String> {
    context
        .kernel_config()
        .map(|config| config.memory.workspace_dir.clone())
        .filter(|value| !value.trim().is_empty())
}

fn resolve_search_defaults(context: &ToolContext) -> (usize, Option<f32>) {
    let max_results = context
        .kernel_config()
        .map(|config| config.memory.search_max_results)
        .unwrap_or(6)
        .max(1);
    let min_score = context
        .kernel_config()
        .map(|config| config.memory.search_min_score);
    (max_results, min_score)
}

fn disabled_output(reason: impl Into<String>) -> ToolOutput {
    ToolOutput {
        success: true,
        output: serde_json::json!({
            "disabled": true,
            "error": reason.into(),
        })
        .to_string(),
        error: None,
    }
}

pub struct MemorySearchTool;

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory_search"
    }

    fn description(&self) -> &str {
        "Searches memory/*.md for prior work, decisions, dates, preferences, and todos. Use this before answering memory-dependent questions."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {"type": "string"},
                "max_results": {"type": "integer", "minimum": 1},
                "min_score": {"type": "number", "minimum": 0.0, "maximum": 1.0}
            }
        }))
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let Some(workspace_dir) = resolve_workspace_dir(&context) else {
            return Ok(disabled_output("memory workspace unavailable"));
        };
        let query = input
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if query.is_empty() {
            return Ok(ToolOutput {
                success: false,
                output: String::new(),
                error: Some("query is required".to_string()),
            });
        }

        let (default_max_results, default_min_score) = resolve_search_defaults(&context);
        let max_results = input
            .get("max_results")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(default_max_results)
            .max(1);
        let min_score = input
            .get("min_score")
            .and_then(Value::as_f64)
            .map(|value| value as f32)
            .or(default_min_score);

        let memory = MemoryStore::new(workspace_dir);
        let results = memory
            .search_memory(&MemorySearchQuery {
                query,
                max_results,
                min_score,
            })
            .await?;
        let output = serde_json::to_string(&MemorySearchResponse { results }).map_err(|err| {
            TalosError::ToolError(format!("serialize memory_search result: {}", err))
        })?;

        Ok(ToolOutput {
            success: true,
            output,
            error: None,
        })
    }
}

pub struct MemoryGetTool;

#[async_trait]
impl Tool for MemoryGetTool {
    fn name(&self) -> &str {
        "memory_get"
    }

    fn description(&self) -> &str {
        "Reads a precise snippet from memory/*.md after memory_search returns a path."
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {"type": "string"},
                "from": {"type": "integer", "minimum": 1},
                "lines": {"type": "integer", "minimum": 1}
            }
        }))
    }

    async fn execute(&self, input: Value, context: ToolContext) -> TalosResult<ToolOutput> {
        let Some(workspace_dir) = resolve_workspace_dir(&context) else {
            return Ok(disabled_output("memory workspace unavailable"));
        };
        let path = input
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if path.is_empty() {
            return Ok(ToolOutput {
                success: false,
                output: String::new(),
                error: Some("path is required".to_string()),
            });
        }
        let from = input
            .get("from")
            .and_then(Value::as_u64)
            .map(|value| value as usize);
        let lines = input
            .get("lines")
            .and_then(Value::as_u64)
            .map(|value| value as usize);

        let memory = MemoryStore::new(workspace_dir);
        let result: MemoryGetResult = memory.get_memory(&path, from, lines).await?;
        let output = serde_json::to_string(&result).map_err(|err| {
            TalosError::ToolError(format!("serialize memory_get result: {}", err))
        })?;
        Ok(ToolOutput {
            success: true,
            output,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use talos_config::AgentKernelConfig;

    fn temp_workspace_dir() -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        std::env::temp_dir().join(format!("talos-memory-tool-{unique}"))
    }

    fn test_kernel_config(workspace_dir: &std::path::Path) -> Arc<AgentKernelConfig> {
        let mut config = AgentKernelConfig::default();
        config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
        config.memory.search_min_score = 0.0;
        Arc::new(config)
    }

    #[tokio::test]
    async fn test_memory_search_uses_structured_defaults() {
        let workspace_dir = temp_workspace_dir();
        let _memory = MemoryStore::new(workspace_dir.clone());
        tokio::fs::create_dir_all(workspace_dir.join("memory"))
            .await
            .expect("memory dir should create");
        tokio::fs::write(
            workspace_dir.join("memory").join("2026-03-25.md"),
            "## Decisions\nrelease target is friday\n",
        )
        .await
        .expect("memory file should write");

        let tool = MemorySearchTool;
        let context = ToolContext::new_with_kernel_config(
            "session-1",
            "user-1",
            Some(test_kernel_config(&workspace_dir)),
        );

        let output = tool
            .execute(serde_json::json!({ "query": "friday" }), context)
            .await
            .expect("tool should execute");
        let response: MemorySearchResponse =
            serde_json::from_str(&output.output).expect("response should deserialize");

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].path, "memory/2026-03-25.md");
    }

    #[tokio::test]
    async fn test_memory_search_input_overrides_structured_defaults() {
        let workspace_dir = temp_workspace_dir();
        let _memory = MemoryStore::new(workspace_dir.clone());
        tokio::fs::create_dir_all(workspace_dir.join("memory"))
            .await
            .expect("memory dir should create");
        tokio::fs::write(
            workspace_dir.join("memory").join("2026-03-25.md"),
            "## Milestone\nfriday checkpoint\n",
        )
        .await
        .expect("memory file should write");
        tokio::fs::write(
            workspace_dir.join("memory").join("2026-03-26.md"),
            "## Follow-up\nfriday delivery\n",
        )
        .await
        .expect("memory file should write");

        let tool = MemorySearchTool;
        let mut config = AgentKernelConfig::default();
        config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
        config.memory.search_max_results = 1;
        config.memory.search_min_score = 1.0;
        let context =
            ToolContext::new_with_kernel_config("session-1", "user-1", Some(Arc::new(config)));

        let output = tool
            .execute(
                serde_json::json!({
                    "query": "friday",
                    "max_results": 4,
                    "min_score": 0.0
                }),
                context,
            )
            .await
            .expect("tool should execute");
        let response: MemorySearchResponse =
            serde_json::from_str(&output.output).expect("response should deserialize");

        assert_eq!(response.results.len(), 2);
    }

    #[tokio::test]
    async fn test_memory_search_expands_tilde_workspace_dir() {
        let (year, month, day) = (2026, 3, 27);
        let daily_file_name = format!("{year:04}-{month:02}-{day:02}.md");
        let expected_memory_path = format!("memory/{daily_file_name}");
        let home = std::env::var("HOME").expect("HOME should be set");
        let home_path = std::path::PathBuf::from(&home);
        let workspace_dir = home_path.join(format!(
            "talos-memory-tool-tilde-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time should be valid")
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&workspace_dir)
            .await
            .expect("workspace dir should create");
        tokio::fs::create_dir_all(workspace_dir.join("memory"))
            .await
            .expect("memory dir should create");
        tokio::fs::write(
            workspace_dir.join("memory").join(&daily_file_name),
            "## Notes\nTea preference is durable.\n",
        )
        .await
        .expect("daily memory should write");

        let tool = MemorySearchTool;
        let mut config = AgentKernelConfig::default();
        let relative = workspace_dir
            .strip_prefix(&home_path)
            .expect("workspace should live under HOME");
        config.memory.workspace_dir = format!("~/{}", relative.to_string_lossy());
        config.memory.search_min_score = 0.0;
        let context =
            ToolContext::new_with_kernel_config("session-1", "user-1", Some(Arc::new(config)));

        let output = tool
            .execute(serde_json::json!({ "query": "tea" }), context)
            .await
            .expect("tool should execute");
        let response: MemorySearchResponse =
            serde_json::from_str(&output.output).expect("response should deserialize");

        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].path, expected_memory_path);

        let _ = tokio::fs::remove_dir_all(workspace_dir).await;
    }
}
