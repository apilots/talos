use super::*;
use std::sync::Arc;
use talos_config::AgentKernelConfig;

fn test_kernel_config(workspace_dir: &std::path::Path) -> Arc<AgentKernelConfig> {
    let mut config = AgentKernelConfig::default();
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    Arc::new(config)
}

#[tokio::test]
async fn test_memory_get_errors_without_memory_store() {
    let workspace_dir = std::env::temp_dir().join(format!(
        "talos-memory-get-tool-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos()
    ));
    let tool = MemoryGetTool;
    let context = ToolContext::new_with_kernel_config(
        "session-1",
        "user-1",
        Some(test_kernel_config(&workspace_dir)),
    );
    let error = tool
        .execute_payload(
            serde_json::json!({ "path": "memory/2026-03-25.md" }),
            context,
        )
        .await
        .expect_err("tool should fail when memory store is missing");
    assert!(matches!(error, TalosError::ToolError(_)));
}
