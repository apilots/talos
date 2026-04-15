use super::*;
use chrono::{Local, Utc};
use serde_json::json;
use std::future::pending;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use talos_agent_kernel::StopReason as KernelStopReason;
use talos_agent_kernel::{KernelBuilder, RunRequest, RunResponse, Runner};
use talos_config::AgentKernelConfig;
use talos_llm_provider::{
    ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider, Role, Usage,
};
use talos_memory::{Memory, TalosTurn};
use talos_tools::{ReadFileTool, ToolRegistry, WebSearchTool};
use talos_tools_policy::ToolPolicy;
use talos_types::talos::{
    StopReason as TalosStopReason, TalosInputItem, TalosOutputItem, TalosProvider, TalosSourceType,
    TalosUsage,
};
use talos_types::{SessionContext, SessionState, TalosSession};
use tempfile::TempDir;
use tokio::sync::Notify;

fn build_test_agent() -> AgentImpl {
    AgentImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        Arc::new(MockLlmProvider {}),
        "gpt-4o".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    )
}

struct SequencedRunner {
    started: Arc<Notify>,
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl Runner for SequencedRunner {
    async fn run_turn(
        &self,
        _agent: Arc<dyn talos_agent_kernel::Agent>,
        _request: RunRequest,
    ) -> talos_error::TalosResult<RunResponse> {
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
        if call_index == 0 {
            self.started.notify_waiters();
            pending::<()>().await;
            unreachable!("pending future should not resolve");
        }

        Ok(RunResponse {
            output_text: "runner ok".to_string(),
            output_items: vec![TalosOutputItem::OutputText {
                text: "runner ok".to_string(),
            }],
            stop_reason: KernelStopReason::EndTurn,
            usage: TalosUsage::default(),
            warnings: Vec::new(),
            model: "mock-model".to_string(),
            task_results: Vec::new(),
            transfer_to: None,
            iterations: 1,
            events: Vec::new(),
        })
    }
}

fn build_temp_agent() -> (TempDir, AgentImpl) {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let mut kernel_config = AgentKernelConfig::default();
    kernel_config.memory.workspace_dir = temp_dir.path().display().to_string();
    kernel_config.memory.compaction_threshold_bytes = usize::MAX;
    let agent = AgentImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        Arc::new(MockLlmProvider {}),
        "gpt-4o".to_string(),
        Arc::new(ToolRegistry::new()),
        kernel_config,
    );
    (temp_dir, agent)
}

fn build_test_session(session_id: &str, workspace_id: &str) -> TalosSession {
    TalosSession {
        title: "Untitled Session".to_string(),
        context: SessionContext {
            session_id: session_id.to_string(),
            workspace_id: Some(workspace_id.to_string()),
            source_type: TalosSourceType::Cli,
            provider: TalosProvider::Cli,
            user_id: "local-user".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: None,
            thread_id: None,
            environment: None,
        },
        config: talos_config::Config::default(),
        state: SessionState {
            created_at: 1,
            last_activity: 1,
            message_count: 2,
            usage: TalosUsage::default(),
        },
    }
}

#[tokio::test]
async fn test_process_tool_calls() {
    let mock_provider = MockLlmProvider {};
    let policy = ToolPolicy::allow_list(vec!["web_search"]);
    let registry = Arc::new(ToolRegistry::new_with_policy(policy));
    registry.register(Arc::new(WebSearchTool));
    let agent = AgentImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        Arc::new(mock_provider),
        "gpt-4o".to_string(),
        registry,
        Default::default(),
    );

    let tool_call = FunctionCall {
        id: "call_123".to_string(),
        name: "web_search".to_string(),
        arguments: json!({"query": "test"}),
    };

    let result = agent.process_tool_calls(vec![tool_call]).await;

    assert!(result.is_ok());
    let messages = result.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, Role::Tool);
}

#[tokio::test]
async fn test_generate_system_prompt() {
    let mock_provider = MockLlmProvider {};
    let agent = AgentImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        Arc::new(mock_provider),
        "gpt-4o".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );

    agent.add_tool(Arc::new(WebSearchTool));
    agent.add_tool(Arc::new(ReadFileTool));

    let prompt = agent.generate_system_prompt();

    assert!(prompt.contains("Test Agent"));
    assert!(prompt.contains("An agent for testing"));
    assert!(prompt.contains("web_search"));
    assert!(prompt.contains("read_file"));
}

#[tokio::test]
async fn test_add_tool() {
    let mock_provider = MockLlmProvider {};
    let agent = AgentImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        Arc::new(mock_provider),
        "gpt-4o".to_string(),
        Arc::new(ToolRegistry::new()),
        Default::default(),
    );

    assert_eq!(agent.tool_registry.list_tool_names().len(), 0);

    agent.add_tool(Arc::new(WebSearchTool));

    let tools = agent.tool_registry.list_tool_names();
    assert!(tools.contains(&"web_search".to_string()));
}

#[tokio::test]
async fn test_execute_tool() {
    let mock_provider = MockLlmProvider {};
    let policy = ToolPolicy::allow_list(vec!["web_search"]);
    let registry = Arc::new(ToolRegistry::new_with_policy(policy));
    registry.register(Arc::new(WebSearchTool));
    let agent = AgentImpl::new(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        Arc::new(mock_provider),
        "gpt-4o".to_string(),
        registry,
        Default::default(),
    );

    let result = agent
        .execute_tool("web_search", json!({"query": "test"}))
        .await;

    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(response.output_text.contains("Search results for 'test'"));
}

#[tokio::test]
async fn test_close_session_flushes_workspace_memory() {
    let (_guard, agent) = build_temp_agent();
    let session_id = "session-close-flush";
    let session = build_test_session(session_id, "workspace-a");
    let timestamp = Utc::now();
    let turn = TalosTurn {
        turn_id: "turn-1".to_string(),
        session_id: session_id.to_string(),
        seq: 1,
        timestamp,
        items: vec![
            TalosOutputItem::UserInput {
                text: "Remember that I prefer tea over coffee.".to_string(),
            },
            TalosOutputItem::OutputText {
                text: "Noted. You prefer tea over coffee.".to_string(),
            },
        ],
        usage: TalosUsage::default(),
        stop_reason: TalosStopReason::EndTurn,
    };

    agent
        .memory_store
        .append_turn(&turn)
        .await
        .expect("session turn should be persisted");

    agent
        .close_session(session)
        .await
        .expect("session close should enqueue workspace memory flush");
    agent
        .drain_pending_memory_flushes(Duration::from_secs(5))
        .await
        .expect("memory flush should complete during drain");

    let file_path = _guard
        .path()
        .join("memory")
        .join(format!("{}.md", Local::now().format("%Y-%m-%d")));
    let mut content = None;
    for _ in 0..20 {
        match tokio::fs::read_to_string(&file_path).await {
            Ok(value) => {
                content = Some(value);
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(25)).await,
        }
    }
    let content = content.expect("workspace daily memory file should exist");
    assert!(content.contains("## ["));
    assert!(content.contains("session:"));
    assert!(content.contains("Durable Memory"));
    assert!(!content.trim().is_empty());
}

#[test]
fn test_derive_exec_command() {
    assert_eq!(AgentImpl::derive_exec_command("run echo ok"), "echo ok");
    assert_eq!(AgentImpl::derive_exec_command("execute ls"), "ls");
    assert_eq!(AgentImpl::derive_exec_command("exec pwd"), "pwd");
    assert_eq!(AgentImpl::derive_exec_command("echo plain"), "echo plain");
}

#[test]
fn test_planner_mode_comes_from_kernel_config() {
    let mut agent = build_test_agent();
    agent
        .set_default_planner_mode("plan_react")
        .expect("planner mode should parse");
    assert_eq!(
        agent.kernel_config.planner_mode,
        talos_config::PlannerMode::PlanReAct
    );
}

#[test]
fn test_kernel_binding_default_is_llm() {
    let agent = build_test_agent();
    assert_eq!(agent.kernel_config.binding.default_agent_kind, "llm");
}

#[tokio::test]
async fn test_submit_dynamic_tool_result_rejects_replay() {
    let (_temp_dir, agent) = build_temp_agent();
    let session_id = "session-dynamic-replay";
    let request_record = talos_memory::SessionTranscriptRecord {
        header: talos_memory::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: uuid::Uuid::new_v4().to_string(),
            branch: talos_memory::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now(),
        },
        author: talos_memory::SessionTranscriptAuthor::System,
        kind: talos_memory::SessionTranscriptEventKind::SystemMessage,
        payload: talos_memory::SessionTranscriptPayload::DynamicToolRequest {
            payload: talos_memory::SessionTranscriptDynamicToolRequestPayload {
                call_id: "call-1".to_string(),
                tool_name: "skill_greet".to_string(),
                arguments: json!({ "name": "Talos" }),
            },
        },
        actions: talos_memory::SessionTranscriptActions::default(),
        partial: false,
    };
    agent
        .memory_store
        .append_transcript_record(&request_record)
        .await
        .expect("request record should append");

    agent
        .submit_dynamic_tool_result(
            session_id,
            "call-1",
            "skill_greet",
            "hello".to_string(),
            true,
        )
        .await
        .expect("first submission should succeed");

    let error = agent
        .submit_dynamic_tool_result(
            session_id,
            "call-1",
            "skill_greet",
            "hello again".to_string(),
            true,
        )
        .await
        .expect_err("second submission should be rejected");
    assert!(error.to_string().contains("already been resolved"));
}

#[tokio::test]
async fn test_submit_dynamic_tool_result_rejects_timeout() {
    let (_temp_dir, mut agent) = build_temp_agent();
    agent.kernel_config.tools.dynamic.response_timeout_ms = 1;
    let session_id = "session-dynamic-timeout";
    let request_record = talos_memory::SessionTranscriptRecord {
        header: talos_memory::SessionTranscriptHeader {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            invocation_id: uuid::Uuid::new_v4().to_string(),
            branch: talos_memory::SessionTranscriptBranch::ReactiveLoop,
            timestamp: Utc::now() - chrono::Duration::milliseconds(10),
        },
        author: talos_memory::SessionTranscriptAuthor::System,
        kind: talos_memory::SessionTranscriptEventKind::SystemMessage,
        payload: talos_memory::SessionTranscriptPayload::DynamicToolRequest {
            payload: talos_memory::SessionTranscriptDynamicToolRequestPayload {
                call_id: "call-timeout".to_string(),
                tool_name: "skill_greet".to_string(),
                arguments: json!({}),
            },
        },
        actions: talos_memory::SessionTranscriptActions::default(),
        partial: false,
    };
    agent
        .memory_store
        .append_transcript_record(&request_record)
        .await
        .expect("request record should append");

    let error = agent
        .submit_dynamic_tool_result(
            session_id,
            "call-timeout",
            "skill_greet",
            "late".to_string(),
            true,
        )
        .await
        .expect_err("timed out submission should be rejected");
    assert!(error.to_string().contains("timed out"));
}

#[tokio::test]
async fn test_cancel_active_turn_aborts_only_current_session_turn() {
    let mut agent = build_test_agent();
    let started = Arc::new(Notify::new());
    agent.kernel_runtime = Arc::new(
        KernelBuilder::new()
            .with_runner(Arc::new(SequencedRunner {
                started: started.clone(),
                calls: AtomicUsize::new(0),
            }))
            .build(),
    );
    let session = build_test_session("session-cancel", "workspace-a");

    let mut first_stream = agent
        .run_stream(OrchestratorRequest {
            session: session.clone(),
            input: vec![TalosInputItem::Text {
                text: "first".to_string(),
            }],
        })
        .await
        .expect("first stream should start");
    started.notified().await;
    agent
        .cancel_active_turn(&session.context.session_id)
        .await
        .expect("cancel should succeed");

    let mut first_stop_reason = None;
    while let Some(event) = first_stream.recv().await {
        if let OrchestratorEvent::MessageCompleted(payload) = event {
            first_stop_reason = Some(payload.stop_reason);
            break;
        }
    }
    assert_eq!(first_stop_reason, Some(TalosStopReason::Cancelled));

    let mut second_stream = agent
        .run_stream(OrchestratorRequest {
            session,
            input: vec![TalosInputItem::Text {
                text: "second".to_string(),
            }],
        })
        .await
        .expect("second stream should start");
    let mut second_stop_reason = None;
    while let Some(event) = second_stream.recv().await {
        if let OrchestratorEvent::MessageCompleted(payload) = event {
            second_stop_reason = Some(payload.stop_reason);
            break;
        }
    }
    assert_eq!(second_stop_reason, Some(TalosStopReason::EndTurn));
}

struct MockLlmProvider;

#[async_trait::async_trait]
impl LlmProvider for MockLlmProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn chat(&self, request: ChatRequest) -> talos_error::TalosResult<ChatResponse> {
        let content = if request.messages.iter().any(|message| {
            message
                .content
                .contains("Extract durable memory from this conversation as JSON.")
        }) {
            "{\"title\":\"Durable Memory\",\"facts\":[\"User prefers tea over coffee\"]}"
                .to_string()
        } else {
            "Test response".to_string()
        };
        Ok(ChatResponse {
            content,
            reasoning: Vec::new(),
            model: "mock-model".to_string(),
            usage: Usage { ..Usage::default() },
            finish_reason: None,
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> talos_error::TalosResult<ChatStream> {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        drop(tx);
        Ok(ChatStream { rx })
    }
}
