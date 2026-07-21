use super::*;
use std::future::Future;
use std::pin::Pin;
use talos_memory::{MemoryReader, MemoryScope};
use talos_types::{
    MemoryGetRequest, MemoryGetResponse, MemoryImportance, MemoryInfo, MemoryKind, MemorySearchHit,
    MemorySearchRequest, MemorySearchResponse, MemorySourceInfo,
};

const MEMORY_BODY: &str = "The user prefers concise architecture proposals.";

struct TestMemoryReader;

impl MemoryReader for TestMemoryReader {
    fn search<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        _request: MemorySearchRequest,
    ) -> Pin<Box<dyn Future<Output = TalosResult<MemorySearchResponse>> + Send + 'a>> {
        Box::pin(async {
            Ok(MemorySearchResponse {
                data: vec![MemorySearchHit {
                    memory_id: "mem-1".to_string(),
                    title: "Architecture preference".to_string(),
                    preview: "Concise architecture".to_string(),
                    kind: MemoryKind::Preference,
                    importance: MemoryImportance::High,
                    occurred_at: 1,
                    rank: 1,
                    source: memory_source(),
                }],
            })
        })
    }

    fn get<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        _request: MemoryGetRequest,
    ) -> Pin<Box<dyn Future<Output = TalosResult<MemoryGetResponse>> + Send + 'a>> {
        Box::pin(async {
            Ok(MemoryGetResponse {
                memory: MemoryInfo {
                    memory_id: "mem-1".to_string(),
                    title: "Architecture preference".to_string(),
                    body: MEMORY_BODY.to_string(),
                    kind: MemoryKind::Preference,
                    importance: MemoryImportance::High,
                    occurred_at: 1,
                    source: memory_source(),
                },
            })
        })
    }
}

#[derive(Default)]
struct MemoryToolLoopProvider {
    round: AtomicUsize,
}

#[async_trait]
impl LlmProvider for MemoryToolLoopProvider {
    fn name(&self) -> &str {
        "memory-tool-loop"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let round = self.round.fetch_add(1, Ordering::SeqCst);
        let tool_call = match round {
            0 => Some(FunctionCall::new(
                "search-memory",
                "MemorySearch",
                json!({ "query": "architecture" }),
            )),
            1 => {
                let search_result = request
                    .messages
                    .iter()
                    .find(|message| message.tool_call_id.as_deref() == Some("search-memory"))
                    .expect("search result should reach the next model round");
                assert!(search_result.content.contains("mem-1"));
                assert!(search_result.content.contains("Concise architecture"));
                assert!(!request
                    .messages
                    .iter()
                    .any(|message| message.content.contains("Retrieved memories:")));
                Some(FunctionCall::new(
                    "get-memory",
                    "MemoryGet",
                    json!({ "memoryId": "mem-1" }),
                ))
            }
            2 => {
                let get_result = request
                    .messages
                    .iter()
                    .find(|message| message.tool_call_id.as_deref() == Some("get-memory"))
                    .expect("get result should reach the next model round");
                assert!(get_result.content.contains(MEMORY_BODY));
                assert_eq!(
                    request
                        .messages
                        .iter()
                        .filter(|message| message.content.contains(MEMORY_BODY))
                        .count(),
                    1
                );
                None
            }
            other => panic!("unexpected memory model round {other}"),
        };
        Ok(ChatResponse {
            content: if tool_call.is_some() {
                String::new()
            } else {
                "Memory used once.".to_string()
            },
            reasoning: ReasoningOutput::default(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some(if tool_call.is_some() {
                "tool_calls".to_string()
            } else {
                "stop".to_string()
            }),
            tool_calls: tool_call.map(|call| vec![call]),
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "memory-tool-loop".to_string()
    }
}

#[tokio::test]
async fn search_then_get_injects_only_the_complete_memory() {
    let provider = Arc::new(MemoryToolLoopProvider::default());
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());
    tool_registry.set_memory_reader(Arc::new(TestMemoryReader));
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = Arc::new(TestMemoryStore::new(workspace_dir.clone()));
    let mut config = AgentKernelConfig::default();
    config.prompt.bootstrap.enabled = false;
    config.workspace_dir = workspace_dir.to_string_lossy().to_string();
    configure_test_skill_root(&mut config, &workspace_dir);
    let mut ctx = build_ctx_with_stream_for_session("memory-tool-loop", None);

    let result = run_general_turn(
        &mut ctx,
        "talos",
        "assistant",
        "Use long-term memory for my architecture preference.",
        provider.clone(),
        tool_registry.clone(),
        tool_registry,
        memory_store,
        &config,
        skill_options_from_config(&workspace_dir, &config),
    )
    .await
    .expect("memory tool loop should succeed");

    assert_eq!(result.output_text, "Memory used once.");
    assert_eq!(provider.round.load(Ordering::SeqCst), 3);
}

fn memory_source() -> MemorySourceInfo {
    MemorySourceInfo {
        date: "2026-07-17".to_string(),
        session_id: Some("session-1".to_string()),
        from_seq: Some(1),
        until_seq: Some(2),
    }
}
