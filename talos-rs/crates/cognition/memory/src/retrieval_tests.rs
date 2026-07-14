use super::*;
use async_trait::async_trait;
use chrono::Utc;
use talos_error::TalosResult;
use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider, Usage};
use tempfile::TempDir;

use crate::{Memory, WorkspaceMemoryRecord};

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
        let content = responses.remove(0);
        Ok(ChatResponse {
            content,
            reasoning: Default::default(),
            model: "mock".to_string(),
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: Some(Vec::<FunctionCall>::new()),
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        unreachable!("stream is not used in tests")
    }

    fn default_model(&self) -> String {
        "mock-model".to_string()
    }
}

fn new_temp_store_with_llm(responses: Vec<String>) -> (TempDir, MemoryStore) {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let llm = Arc::new(MockLlmProvider::new(responses));
    let store = MemoryStore::new_with_llm(temp_dir.path().to_path_buf(), llm);
    (temp_dir, store)
}

#[tokio::test]
async fn test_rebuild_file_index_extracts_sections() {
    let (_guard, store) = new_temp_store_with_llm(Vec::new());
    let timestamp = Utc::now();
    store
        .append_workspace_memory(&WorkspaceMemoryRecord {
            timestamp,
            session_id: "session-1".to_string(),
            title: "Flush required".to_string(),
            content: "**Facts:**\n- flush on close\n\n**Preferences:**\n- ephemeral sessions\n"
                .to_string(),
        })
        .await
        .expect("workspace memory append should succeed");

    let date = timestamp
        .with_timezone(&store.timezone())
        .format("%Y-%m-%d")
        .to_string();
    let index = rebuild_file_index(&store, &date)
        .await
        .expect("rebuild should succeed");
    assert_eq!(index.sections.len(), 1);
    assert_eq!(index.sections[0].title, "Flush required");
    assert_eq!(index.sections[0].session_id, "session-1");
    assert!(index.sections[0].summary_preview.contains("flush on close"));
}

#[tokio::test]
async fn test_rebuild_all_indexes_bootstraps_existing_md_files() {
    let (_guard, store) = new_temp_store_with_llm(Vec::new());
    let memory_dir = store.workspace_memory_dir();
    talos_fs::create_directory(&memory_dir, true).expect("memory dir should exist");
    talos_fs::write_file(
        &memory_dir.join("2026-04-04.md"),
        "## [10:41] Flush required\nsession: session-1\n\n**Facts:**\n- keep no residue\n",
    )
    .await
    .expect("seed memory file should write");

    rebuild_all_indexes(&store)
        .await
        .expect("rebuild all should succeed");

    let loaded = load_index_files(&store)
        .await
        .expect("load indexes should succeed");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].path, "memory/2026-04-04.md");
}

#[tokio::test]
async fn test_semantic_search_uses_two_hops() {
    let (_guard, store) = new_temp_store_with_llm(vec![
        "[\"memory/2026-04-04.md\"]".to_string(),
        "[{\"path\":\"memory/2026-04-04.md\",\"from\":1,\"to\":5}]".to_string(),
    ]);
    let memory_dir = store.workspace_memory_dir();
    talos_fs::create_directory(&memory_dir, true).expect("memory dir should exist");
    talos_fs::write_file(
        &memory_dir.join("2026-04-04.md"),
        "## [10:41] Flush required\nsession: session-1\n\n**Facts:**\n- flush on close\n",
    )
    .await
    .expect("seed memory file should write");
    rebuild_all_indexes(&store)
        .await
        .expect("rebuild all should succeed");

    let results = store
        .search_memory(&MemorySearchQuery {
            query: "flush".to_string(),
            max_results: 3,
        })
        .await
        .expect("search should succeed");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].path, "memory/2026-04-04.md");
    assert!(results[0].snippet.contains("flush on close"));
}

#[tokio::test]
async fn test_semantic_search_rejects_hop2_refs_outside_candidate_sections() {
    let (_guard, store) = new_temp_store_with_llm(vec![
            "[\"memory/2026-04-04.md\"]".to_string(),
            "[{\"path\":\"memory/2026-04-05.md\",\"from\":1,\"to\":6},{\"path\":\"memory/2026-04-04.md\",\"from\":1,\"to\":99}]".to_string(),
        ]);
    let memory_dir = store.workspace_memory_dir();
    talos_fs::create_directory(&memory_dir, true).expect("memory dir should exist");
    talos_fs::write_file(
        &memory_dir.join("2026-04-04.md"),
        "## [10:41] Flush required\nsession: session-1\n\n**Facts:**\n- flush on close\n",
    )
    .await
    .expect("seed memory file should write");
    talos_fs::write_file(
        &memory_dir.join("2026-04-05.md"),
        "## [11:01] Unrelated\nsession: session-2\n\n**Facts:**\n- unrelated content\n",
    )
    .await
    .expect("second seed memory file should write");
    rebuild_all_indexes(&store)
        .await
        .expect("rebuild all should succeed");

    let results = store
        .search_memory(&MemorySearchQuery {
            query: "flush".to_string(),
            max_results: 3,
        })
        .await
        .expect("search should succeed");
    assert!(results.is_empty(), "invalid hop2 refs should be discarded");
}

#[tokio::test]
async fn test_semantic_search_ignores_hop2_refs_with_inverted_range() {
    let (_guard, store) = new_temp_store_with_llm(vec![
        "[\"memory/2026-04-04.md\"]".to_string(),
        "[{\"path\":\"memory/2026-04-04.md\",\"from\":5,\"to\":1}]".to_string(),
    ]);
    let memory_dir = store.workspace_memory_dir();
    talos_fs::create_directory(&memory_dir, true).expect("memory dir should exist");
    talos_fs::write_file(
        &memory_dir.join("2026-04-04.md"),
        "## [10:41] Flush required\nsession: session-1\n\n**Facts:**\n- flush on close\n",
    )
    .await
    .expect("seed memory file should write");
    rebuild_all_indexes(&store)
        .await
        .expect("rebuild all should succeed");

    let results = store
        .search_memory(&MemorySearchQuery {
            query: "flush".to_string(),
            max_results: 3,
        })
        .await
        .expect("search should succeed");
    assert!(results.is_empty(), "inverted hop2 refs should be ignored");
}

#[tokio::test]
async fn test_semantic_search_errors_on_invalid_hop2_json() {
    let (_guard, store) = new_temp_store_with_llm(vec![
        "[\"memory/2026-04-04.md\"]".to_string(),
        "{\"path\":\"memory/2026-04-04.md\"}".to_string(),
    ]);
    let memory_dir = store.workspace_memory_dir();
    talos_fs::create_directory(&memory_dir, true).expect("memory dir should exist");
    talos_fs::write_file(
        &memory_dir.join("2026-04-04.md"),
        "## [10:41] Flush required\nsession: session-1\n\n**Facts:**\n- flush on close\n",
    )
    .await
    .expect("seed memory file should write");
    rebuild_all_indexes(&store)
        .await
        .expect("rebuild all should succeed");

    let error = store
        .search_memory(&MemorySearchQuery {
            query: "flush".to_string(),
            max_results: 3,
        })
        .await
        .expect_err("invalid hop2 json should fail");
    assert!(matches!(error, TalosError::ToolError(_)));
}

#[tokio::test]
async fn test_semantic_search_requires_llm_backed_store() {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let store = MemoryStore::new(temp_dir.path().to_path_buf());
    let result = store
        .search_memory(&MemorySearchQuery {
            query: "anything".to_string(),
            max_results: 1,
        })
        .await;
    assert!(matches!(result, Err(TalosError::ToolError(_))));
}

#[test]
fn test_strip_json_fences_supports_markdown_wrappers() {
    assert_eq!(strip_json_fences("```json\n[1,2]\n```"), "[1,2]");
    assert_eq!(strip_json_fences("```\n{\"a\":1}\n```"), "{\"a\":1}");
}
