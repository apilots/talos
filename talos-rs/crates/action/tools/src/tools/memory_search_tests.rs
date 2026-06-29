use super::*;
use async_trait::async_trait;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_llm_provider::{
    ChatRequest, ChatResponse, ChatStream, LlmProvider, ReasoningSupport, Usage,
};
use talos_memory::MemoryStore;

#[derive(Clone)]
struct MockLlmProvider {
    responses: Arc<tokio::sync::Mutex<Vec<String>>>,
}

impl MockLlmProvider {
    fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Arc::new(tokio::sync::Mutex::new(responses)),
        }
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    fn name(&self) -> &str {
        "mock"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        let mut responses = self.responses.lock().await;
        Ok(ChatResponse {
            content: responses.remove(0),
            reasoning: Vec::new(),
            model: "mock-model".to_string(),
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        unreachable!("stream is not used in tests")
    }

    fn default_model(&self) -> String {
        "mock-model".to_string()
    }

    fn reasoning_support(&self, _model: &str) -> ReasoningSupport {
        ReasoningSupport::Unsupported
    }
}

fn temp_workspace_dir() -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!("talos-memory-tool-{unique}"))
}

fn test_kernel_config(workspace_dir: &std::path::Path) -> Arc<AgentKernelConfig> {
    let mut config = AgentKernelConfig::default();
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    Arc::new(config)
}

#[tokio::test]
async fn test_memory_search_uses_structured_defaults() {
    let workspace_dir = temp_workspace_dir();
    let llm = Arc::new(MockLlmProvider::new(vec![
        "[\"memory/2026-03-25.md\"]".to_string(),
        "[{\"path\":\"memory/2026-03-25.md\",\"from\":1,\"to\":5}]".to_string(),
    ]));
    let memory = Arc::new(MemoryStore::new_with_llm(workspace_dir.clone(), llm));
    tokio::fs::create_dir_all(workspace_dir.join("memory"))
        .await
        .expect("memory dir should create");
    tokio::fs::write(
            workspace_dir.join("memory").join("2026-03-25.md"),
            "## [10:41] Release target confirmed\nsession: s1\n\n**Facts:**\n- release target is friday\n",
        )
        .await
        .expect("memory file should write");
    memory
        .rebuild_memory_indexes()
        .await
        .expect("indexes should rebuild");

    let tool = MemorySearchTool;
    let context = ToolContext::new_with_kernel_config(
        "session-1",
        "user-1",
        Some(test_kernel_config(&workspace_dir)),
    )
    .with_memory_store(memory);

    let output = tool
        .execute_payload(serde_json::json!({ "query": "friday" }), context)
        .await
        .expect("tool should execute");
    let response: MemorySearchResponse = serde_json::from_value(match output {
        ToolOutputPayload::Function {
            structured_content, ..
        } => structured_content.expect("structured output should exist"),
        _ => panic!("expected function payload"),
    })
    .expect("response should deserialize");

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].path, "memory/2026-03-25.md");
}

#[tokio::test]
async fn test_memory_search_input_overrides_structured_defaults() {
    let workspace_dir = temp_workspace_dir();
    let llm = Arc::new(MockLlmProvider::new(vec![
            "[\"memory/2026-03-25.md\",\"memory/2026-03-26.md\"]".to_string(),
            "[{\"path\":\"memory/2026-03-25.md\",\"from\":1,\"to\":5},{\"path\":\"memory/2026-03-26.md\",\"from\":1,\"to\":5}]".to_string(),
        ]));
    let memory = Arc::new(MemoryStore::new_with_llm(workspace_dir.clone(), llm));
    tokio::fs::create_dir_all(workspace_dir.join("memory"))
        .await
        .expect("memory dir should create");
    tokio::fs::write(
        workspace_dir.join("memory").join("2026-03-25.md"),
        "## [10:41] Friday checkpoint\nsession: s1\n\n**Facts:**\n- friday checkpoint\n",
    )
    .await
    .expect("memory file should write");
    tokio::fs::write(
        workspace_dir.join("memory").join("2026-03-26.md"),
        "## [10:42] Friday delivery\nsession: s2\n\n**Facts:**\n- friday delivery\n",
    )
    .await
    .expect("memory file should write");
    memory
        .rebuild_memory_indexes()
        .await
        .expect("indexes should rebuild");

    let tool = MemorySearchTool;
    let mut config = AgentKernelConfig::default();
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.memory.search_max_results = 1;
    let context =
        ToolContext::new_with_kernel_config("session-1", "user-1", Some(Arc::new(config)))
            .with_memory_store(memory);

    let output = tool
        .execute_payload(
            serde_json::json!({
                "query": "friday",
                "max_results": 4
            }),
            context,
        )
        .await
        .expect("tool should execute");
    let response: MemorySearchResponse = serde_json::from_value(match output {
        ToolOutputPayload::Function {
            structured_content, ..
        } => structured_content.expect("structured output should exist"),
        _ => panic!("expected function payload"),
    })
    .expect("response should deserialize");

    assert_eq!(response.results.len(), 2);
}

#[tokio::test]
async fn test_memory_search_expands_tilde_workspace_dir() {
    let (year, month, day) = (2026, 3, 27);
    let daily_file_name = format!("{year:04}-{month:02}-{day:02}.md");
    let expected_memory_path = format!("memory/{daily_file_name}");
    let home_path = dirs::home_dir().expect("HOME should be set");
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

    let llm = Arc::new(MockLlmProvider::new(vec![
        format!("[\"{}\"]", expected_memory_path),
        format!(
            "[{{\"path\":\"{}\",\"from\":1,\"to\":2}}]",
            expected_memory_path
        ),
    ]));
    let tool = MemorySearchTool;
    let mut config = AgentKernelConfig::default();
    let relative = workspace_dir
        .strip_prefix(&home_path)
        .expect("workspace should live under HOME");
    config.workspace_dir = format!("~/{}", relative.to_string_lossy());
    let context =
        ToolContext::new_with_kernel_config("session-1", "user-1", Some(Arc::new(config)))
            .with_memory_store(Arc::new(MemoryStore::new_with_llm(
                format!("~/{}", relative.to_string_lossy()),
                llm,
            )));

    let output = tool
        .execute_payload(serde_json::json!({ "query": "tea" }), context)
        .await
        .expect("tool should execute");
    let response: MemorySearchResponse = serde_json::from_value(match output {
        ToolOutputPayload::Function {
            structured_content, ..
        } => structured_content.expect("structured output should exist"),
        _ => panic!("expected function payload"),
    })
    .expect("response should deserialize");

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].path, expected_memory_path);

    let _ = tokio::fs::remove_dir_all(workspace_dir).await;
}

#[tokio::test]
async fn test_memory_search_errors_without_memory_store() {
    let workspace_dir = temp_workspace_dir();
    let tool = MemorySearchTool;
    let context = ToolContext::new_with_kernel_config(
        "session-1",
        "user-1",
        Some(test_kernel_config(&workspace_dir)),
    );
    let error = tool
        .execute_payload(serde_json::json!({ "query": "friday" }), context)
        .await
        .expect_err("tool should fail when memory store is missing");
    assert!(matches!(error, TalosError::ToolError(_)));
}
