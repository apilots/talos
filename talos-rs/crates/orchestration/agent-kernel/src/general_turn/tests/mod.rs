use super::*;
use talos_store::session::{SessionArtifactStore, SessionStore, TalosTurn};

struct TestMemoryStore {
    workspace_dir: PathBuf,
    writes: Mutex<Vec<MemoryWriteBatch>>,
    day_entries: Mutex<Vec<talos_memory::DreamMemoryEntry>>,
}

impl TestMemoryStore {
    fn new(workspace_dir: PathBuf) -> Self {
        Self {
            workspace_dir,
            writes: Mutex::new(Vec::new()),
            day_entries: Mutex::new(Vec::new()),
        }
    }

    fn workspace_dir(&self) -> &Path {
        &self.workspace_dir
    }

    fn writes(&self) -> Vec<MemoryWriteBatch> {
        self.writes
            .lock()
            .expect("test memory writes mutex should lock")
            .clone()
    }

    fn set_day_entries(&self, entries: Vec<talos_memory::DreamMemoryEntry>) {
        *self
            .day_entries
            .lock()
            .expect("test day entries mutex should lock") = entries;
    }
}

impl DreamMemoryFormationStore for TestMemoryStore {
    fn read_day<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        date: talos_memory::DreamMemoryDate,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = TalosResult<talos_memory::DreamMemoryDaySnapshot>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            Ok(talos_memory::DreamMemoryDaySnapshot::new(
                date,
                talos_memory::DreamMemoryDayRevision::Missing,
                self.day_entries
                    .lock()
                    .expect("test day entries mutex should lock")
                    .clone(),
            ))
        })
    }

    fn write<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        request: talos_memory::DreamMemoryWriteRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = TalosResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.writes
                .lock()
                .expect("test memory writes mutex should lock")
                .push(request.batch().clone());
            Ok(())
        })
    }
}

struct FailingMemoryWriter;

impl DreamMemoryFormationStore for FailingMemoryWriter {
    fn read_day<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        date: talos_memory::DreamMemoryDate,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = TalosResult<talos_memory::DreamMemoryDaySnapshot>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            Ok(talos_memory::DreamMemoryDaySnapshot::new(
                date,
                talos_memory::DreamMemoryDayRevision::Missing,
                Vec::new(),
            ))
        })
    }

    fn write<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        _request: talos_memory::DreamMemoryWriteRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = TalosResult<()>> + Send + 'a>> {
        Box::pin(async { Err(TalosError::internal("memory write failed".to_string())) })
    }
}

#[async_trait]
trait TestSessionArtifacts {
    async fn next_turn_seq(&self, session_id: &str) -> TalosResult<u64>;
    async fn append_turn(&self, turn: &TalosTurn) -> TalosResult<()>;
    async fn load_summaries(
        &self,
        session_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<SummaryRecord>>;
}

#[async_trait]
impl TestSessionArtifacts for TestMemoryStore {
    async fn next_turn_seq(&self, session_id: &str) -> TalosResult<u64> {
        talos_store::session::FileSessionArtifactStore::default()
            .next_turn_seq(self.workspace_dir(), session_id)
            .await
    }

    async fn append_turn(&self, turn: &TalosTurn) -> TalosResult<()> {
        talos_store::session::FileSessionArtifactStore::default()
            .append_turn(self.workspace_dir(), turn)
            .await
    }

    async fn load_summaries(
        &self,
        session_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<SummaryRecord>> {
        talos_store::session::FileSessionArtifactStore::default()
            .load_summaries(self.workspace_dir(), session_id, limit)
            .await
    }
}
use crate::{
    ContextStateBags, InvocationExecutionState, InvocationLimits, KernelEventData,
    KernelStreamEventData, KernelStreamEventType, ToolExecutionMode, ToolProgressKind, TurnPhase,
};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use talos_config::{resolve_runtime_path, AgentKernelConfig, PluginRootConfig, PluginRootKind};
use talos_llm_provider::{
    ChatChunk, ChatRequest, ChatResponse, ChatStream, FunctionCall, LlmProvider,
    ModelBlockCompleted, ModelBlockDelta, ModelBlockStarted, ModelBlockStream,
    ModelBlockStreamEvent, ReasoningContinuation, ReasoningOutput, Usage,
};
use talos_skills::{SkillRoot, SkillRoots, SkillSelector};
use talos_tools::{register_core_local_tools, ToolRegistry};
use tokio::sync::mpsc;
use uuid::Uuid;

fn missing_user_workspace_memory() -> talos_prompt::UserWorkspaceMemoryPromptContext {
    talos_prompt::UserWorkspaceMemoryPromptContext {
        file_state: talos_prompt::UserWorkspaceMemoryPromptFileState::Missing,
        version_token: None,
        content: String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_general_turn(
    ctx: &mut InvocationContext,
    agent_name: &str,
    agent_description: &str,
    input: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    tool_executor_port: Arc<dyn ToolExecutorPort>,
    dream_memory_store: Arc<dyn DreamMemoryFormationStore>,
    kernel_config: &AgentKernelConfig,
    skill_options: GeneralTurnSkillOptions,
) -> TalosResult<GeneralTurnResult> {
    super::run_general_turn(
        ctx,
        agent_name,
        agent_description,
        input,
        llm_provider,
        tool_definition_provider,
        tool_executor_port,
        dream_memory_store,
        missing_user_workspace_memory(),
        kernel_config,
        skill_options,
        crate::test_run_tasks(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn prepare_general_turn(
    ctx: &mut InvocationContext,
    agent_name: &str,
    agent_description: &str,
    input: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_definition_provider: Arc<dyn ToolDefinitionProvider>,
    dream_memory_store: Arc<dyn DreamMemoryFormationStore>,
    kernel_config: &AgentKernelConfig,
    skill_options: Option<GeneralTurnSkillOptions>,
) -> TalosResult<PreparedGeneralTurn> {
    super::prepare_general_turn(
        ctx,
        agent_name,
        agent_description,
        input,
        llm_provider,
        tool_definition_provider,
        dream_memory_store,
        missing_user_workspace_memory(),
        kernel_config,
        skill_options,
        crate::test_run_tasks(),
    )
    .await
}

mod attachment_tests;
mod compaction_loop_tests;
mod dream_memory_tests;
mod final_response_tests;
mod memory_summary_tests;
mod memory_tool_loop_tests;
mod parsing_tests;
mod persist_tests;
mod prompt_warning_tests;
mod step_request_tests;
mod stream_live_tool_result_tests;
mod stream_tests;
mod tool_loop_tests;

struct NoopLlmProvider;

#[derive(Default)]
struct ExecLoopProvider {
    round: AtomicUsize,
}

struct StreamingToolCallIdProvider;

struct StreamingToolExecutionProvider {
    path: String,
}

struct LengthFinishProvider;

#[derive(Default)]
struct EmptyFinalAfterToolsProvider {
    round: AtomicUsize,
    finalization_context_had_tool_result: AtomicBool,
    dir_path: String,
}

#[derive(Default)]
struct SalvageAfterEmptyFinalProvider {
    round: AtomicUsize,
    dir_path: String,
}

struct SummaryProvider {
    responses: Mutex<VecDeque<String>>,
    requests: Mutex<Vec<ChatRequest>>,
}

fn configure_test_skill_root(config: &mut AgentKernelConfig, workspace_dir: &Path) -> PathBuf {
    let skill_root = workspace_dir.join("agents-skills");
    std::fs::create_dir_all(&skill_root).expect("test skill root should exist");
    config.plugins.roots = vec![PluginRootConfig {
        kind: PluginRootKind::Skill,
        path: skill_root.to_string_lossy().to_string(),
        read_only: true,
    }];
    skill_root
}

fn skill_options_from_config(
    workspace_dir: &Path,
    config: &AgentKernelConfig,
) -> GeneralTurnSkillOptions {
    skill_options_from_config_with_selected(workspace_dir, config, Vec::new())
}

fn skill_options_from_config_with_selected(
    workspace_dir: &Path,
    config: &AgentKernelConfig,
    selected: Vec<SkillSelector>,
) -> GeneralTurnSkillOptions {
    let roots = config
        .plugins
        .roots
        .iter()
        .filter(|root| root.kind == PluginRootKind::Skill && root.read_only)
        .map(|root| SkillRoot::configured(resolve_runtime_path(&root.path)))
        .collect();

    GeneralTurnSkillOptions {
        selected,
        roots: Some(SkillRoots {
            roots,
            state_path: workspace_dir
                .join("plugins")
                .join("skills")
                .join("skills-state.json"),
            max_skill_md_bytes: config.skills.max_skill_md_bytes,
            max_available_summary_bytes: config.skills.max_available_summary_bytes,
        }),
    }
}

impl SummaryProvider {
    fn new(responses: Vec<String>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            requests: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl LlmProvider for NoopLlmProvider {
    fn name(&self) -> &str {
        "noop"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: "ok".to_string(),
            reasoning: ReasoningOutput::default(),
            model: "noop".to_string(),
            usage: Usage {
                prompt_tokens: 1,
                completion_tokens: 1,
                total_tokens: 2,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        tx.send(Ok(ChatChunk {
            delta: "ok".to_string(),
            finish_reason: Some("stop".to_string()),
        }))
        .await
        .expect("noop stream chunk should send");
        Ok(ChatStream::new(rx))
    }

    async fn stream_model_blocks(&self, _request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(4);
        tokio::spawn(async move {
            tx.send(Ok(ModelBlockStreamEvent::BlockStarted(
                ModelBlockStarted::assistant_text(
                    None,
                    talos_llm_provider::ModelOutputPhase::Unknown,
                ),
            )))
            .await
            .expect("assistant start should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockDelta {
                model_item_id: "assistant_text".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "ok".to_string(),
                },
            }))
            .await
            .expect("assistant delta should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockCompleted(
                ModelBlockCompleted {
                    model_item_id: "assistant_text".to_string(),
                },
            )))
            .await
            .expect("assistant completed should send");
            tx.send(Ok(ModelBlockStreamEvent::Done {
                finish_reason: Some("stop".to_string()),
                usage: Some(Usage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                }),
            }))
            .await
            .expect("done should send");
        });
        Ok(ModelBlockStream::new(rx))
    }

    fn default_model(&self) -> String {
        "noop".to_string()
    }
}

#[async_trait]
impl LlmProvider for ExecLoopProvider {
    fn name(&self) -> &str {
        "exec-loop"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        let round = self.round.fetch_add(1, Ordering::SeqCst);
        match round {
            0 => Ok(ChatResponse {
                content: "Run bash and use its final output.".to_string(),
                reasoning: ReasoningOutput::default(),
                model: request.model,
                usage: Usage {
                    prompt_tokens: 60,
                    completion_tokens: 10,
                    total_tokens: 70,
                    ..Usage::default()
                },
                finish_reason: Some("tool_calls".to_string()),
                tool_calls: Some(vec![FunctionCall::new(
                    "call_bash",
                    "Bash",
                    json!({
                        "command": "sleep 0.02; printf 'done:ok\\n'",
                        "description": "Run a short command for verification.",
                        "timeout": 5_000
                    }),
                )]),
            }),
            1 => {
                let tool_message = request
                    .messages
                    .iter()
                    .find(|msg| {
                        msg.role == talos_llm_provider::Role::Tool
                            && msg.tool_call_id.as_deref() == Some("call_bash")
                    })
                    .expect("Bash tool message should exist");
                let payload: Value =
                    serde_json::from_str(&tool_message.content).expect("Bash payload should parse");
                assert_eq!(
                    payload.get("type").and_then(Value::as_str),
                    Some("bash_output")
                );
                assert_eq!(
                    payload.get("status").and_then(Value::as_str),
                    Some("completed")
                );
                let output = payload
                    .get("stdout")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                Ok(ChatResponse {
                    content: format!("Final answer after bash output: {}", output.trim()),
                    reasoning: ReasoningOutput::default(),
                    model: request.model,
                    usage: Usage {
                        prompt_tokens: 90,
                        completion_tokens: 20,
                        total_tokens: 110,
                        ..Usage::default()
                    },
                    finish_reason: Some("stop".to_string()),
                    tool_calls: None,
                })
            }
            _ => panic!("unexpected provider round"),
        }
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "exec-loop".to_string()
    }
}

#[async_trait]
impl LlmProvider for LengthFinishProvider {
    fn name(&self) -> &str {
        "length-finish"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: "partial answer".to_string(),
            reasoning: ReasoningOutput::default(),
            model: "length-finish".to_string(),
            usage: Usage {
                prompt_tokens: 1,
                completion_tokens: 4,
                total_tokens: 5,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
            finish_reason: Some("length".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "length-finish".to_string()
    }
}

#[async_trait]
impl LlmProvider for StreamingToolCallIdProvider {
    fn name(&self) -> &str {
        "streaming-tool-call-id"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        panic!("chat should not be called in stream_round test");
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    async fn stream_model_blocks(&self, _request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(8);
        tokio::spawn(async move {
            tx.send(Ok(ModelBlockStreamEvent::BlockStarted(
                ModelBlockStarted::tool_call(
                    0,
                    Some(String::new()),
                    Some("MemorySearch".to_string()),
                ),
            )))
            .await
            .expect("tool start should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockDelta {
                model_item_id: "tool_call_0".to_string(),
                delta: ModelBlockDelta::ToolArguments {
                    delta: json!({ "query": "talos项目" }).to_string(),
                },
            }))
            .await
            .expect("tool args should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockCompleted(
                ModelBlockCompleted {
                    model_item_id: "tool_call_0".to_string(),
                },
            )))
            .await
            .expect("tool completed should send");
            tx.send(Ok(ModelBlockStreamEvent::Done {
                finish_reason: Some("tool_calls".to_string()),
                usage: None,
            }))
            .await
            .expect("done event should send");
        });
        Ok(ModelBlockStream::new(rx))
    }

    fn default_model(&self) -> String {
        "streaming-tool-call-id".to_string()
    }
}

#[async_trait]
impl LlmProvider for StreamingToolExecutionProvider {
    fn name(&self) -> &str {
        "streaming-tool-execution"
    }

    async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
        panic!("chat should not be called in stream_round test");
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    async fn stream_model_blocks(&self, _request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let (tx, rx) = mpsc::channel::<TalosResult<ModelBlockStreamEvent>>(8);
        let path = self.path.clone();
        tokio::spawn(async move {
            tx.send(Ok(ModelBlockStreamEvent::BlockStarted(
                ModelBlockStarted::assistant_text(
                    None,
                    talos_llm_provider::ModelOutputPhase::Unknown,
                ),
            )))
            .await
            .expect("assistant start should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockDelta {
                model_item_id: "assistant_text".to_string(),
                delta: ModelBlockDelta::AssistantText {
                    delta: "Inspecting a file.".to_string(),
                },
            }))
            .await
            .expect("text delta should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockCompleted(
                ModelBlockCompleted {
                    model_item_id: "assistant_text".to_string(),
                },
            )))
            .await
            .expect("assistant completed should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockStarted(
                ModelBlockStarted::tool_call(
                    0,
                    Some("call_read".to_string()),
                    Some("read".to_string()),
                ),
            )))
            .await
            .expect("tool start should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockDelta {
                model_item_id: "tool_call_0".to_string(),
                delta: ModelBlockDelta::ToolArguments {
                    delta: format!("{{\"path\":\"{}\"}}", path),
                },
            }))
            .await
            .expect("tool delta should send");
            tx.send(Ok(ModelBlockStreamEvent::BlockCompleted(
                ModelBlockCompleted {
                    model_item_id: "tool_call_0".to_string(),
                },
            )))
            .await
            .expect("tool completed should send");
            tx.send(Ok(ModelBlockStreamEvent::Done {
                finish_reason: Some("tool_calls".to_string()),
                usage: None,
            }))
            .await
            .expect("done event should send");
        });
        Ok(ModelBlockStream::new(rx))
    }

    fn default_model(&self) -> String {
        "streaming-tool-execution".to_string()
    }
}

#[async_trait]
impl LlmProvider for EmptyFinalAfterToolsProvider {
    fn name(&self) -> &str {
        "empty-final-after-tools"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        match self.round.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(ChatResponse {
                content: "Inspect the project root first.".to_string(),
                reasoning: ReasoningOutput::default(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("tool_calls".to_string()),
                tool_calls: Some(vec![FunctionCall::new(
                    "call_list",
                    "Glob",
                    json!({
                        "path": self.dir_path,
                        "pattern": "**/*",
                        "limit": 50
                    }),
                )]),
            }),
            1 => Ok(ChatResponse {
                content: String::new(),
                reasoning: ReasoningOutput::default(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            }),
            _ => {
                let has_tool_result = request
                    .messages
                    .iter()
                    .any(|message| message.role == talos_llm_provider::Role::Tool);
                self.finalization_context_had_tool_result
                    .store(has_tool_result, Ordering::SeqCst);
                assert!(
                    has_tool_result,
                    "forced finalization should retain tool results as evidence"
                );
                Ok(ChatResponse {
                    content: "Final architecture summary based on verified tool results."
                        .to_string(),
                    reasoning: ReasoningOutput::default(),
                    model: request.model,
                    usage: Usage::default(),
                    finish_reason: Some("stop".to_string()),
                    tool_calls: None,
                })
            }
        }
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "empty-final-after-tools".to_string()
    }
}

#[async_trait]
impl LlmProvider for SalvageAfterEmptyFinalProvider {
    fn name(&self) -> &str {
        "salvage-after-empty-final"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.chat_with_tools(request).await
    }

    async fn chat_with_tools(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        match self.round.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(ChatResponse {
                content: "Inspect the project root first.".to_string(),
                reasoning: ReasoningOutput::default(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("tool_calls".to_string()),
                tool_calls: Some(vec![FunctionCall::new(
                    "call_list",
                    "Glob",
                    json!({
                        "path": self.dir_path,
                        "pattern": "**/*",
                        "limit": 50
                    }),
                )]),
            }),
            1 | 2 => Ok(ChatResponse {
                content: String::new(),
                reasoning: ReasoningOutput::default(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            }),
            _ => Ok(ChatResponse {
                content: "Salvaged final answer from verified tool results.".to_string(),
                reasoning: ReasoningOutput::default(),
                model: request.model,
                usage: Usage::default(),
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            }),
        }
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "salvage-after-empty-final".to_string()
    }
}

#[async_trait]
impl LlmProvider for SummaryProvider {
    fn name(&self) -> &str {
        "summary-provider"
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        self.requests
            .lock()
            .expect("summary provider requests mutex should lock")
            .push(request);
        let response = self
            .responses
            .lock()
            .expect("summary provider mutex should lock")
            .pop_front()
            .expect("summary provider should have a queued response");
        Ok(ChatResponse {
            content: response,
            reasoning: ReasoningOutput::default(),
            model: "summary-model".to_string(),
            usage: Usage {
                prompt_tokens: 13,
                completion_tokens: 7,
                total_tokens: 20,
                cache_creation_input_tokens: Some(3),
                cache_read_input_tokens: Some(11),
            },
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
        Ok(ChatStream::new(rx))
    }

    fn default_model(&self) -> String {
        "summary-model".to_string()
    }
}

fn build_ctx_with_stream(
    stream_tx: Option<mpsc::Sender<crate::KernelStreamEvent>>,
) -> InvocationContext {
    build_ctx_with_stream_for_session("s1", stream_tx)
}

fn build_ctx_with_stream_for_session(
    session_id: impl Into<String>,
    stream_tx: Option<mpsc::Sender<crate::KernelStreamEvent>>,
) -> InvocationContext {
    let session_id = session_id.into();
    InvocationContext {
        session_id: session_id.clone(),
        user_id: "u1".to_string(),
        message_id: "msg-1".to_string(),
        invocation_id: "i1".to_string(),
        agent_id: "a1".to_string(),
        workspace_id: "w1".to_string(),
        project_root: None,
        input: UserInput {
            text: "hello".to_string(),
            file_ids: Vec::new(),
        },
        resolved_attachments: Vec::new(),
        model_selection: talos_types::ModelSelection::without_reasoning(talos_types::ModelRef {
            provider: "p1".to_string(),
            id: "m1".to_string(),
        }),
        selected_model_info: talos_types::ModelInfo::from_provider_model(
            "p1",
            talos_types::ProviderModelConfig {
                id: "m1".to_string(),
                name: "m1".to_string(),
                context_window: 0,
                reasoning: false,
                reasoning_effort: Default::default(),
                speed: Default::default(),
                input: Vec::new(),
                cost: Default::default(),
            },
        ),
        phase: TurnPhase::Execution,
        planning_instruction: None,
        transfer_to: None,
        execution_state: InvocationExecutionState::default(),
        session: crate::InvocationSession {
            session_id: session_id.clone(),
            ..crate::InvocationSession::default()
        },
        session_environment: None,
        state: HashMap::new(),
        state_bags: ContextStateBags::default(),
        limits: InvocationLimits::default(),
        events: Vec::new(),
        transcript_service: None,
        session_artifact_store: Some(Arc::new(
            talos_store::session::FileSessionArtifactStore::default(),
        )),
        session_store: Some(Arc::new(talos_store::session::MockSessionStore::new())),
        stream_tx,
    }
}

async fn seed_recent_user_message(memory_store: &TestMemoryStore) {
    let timestamp = Utc::now();
    memory_store
        .append_turn(&TalosTurn {
            turn_id: "turn-1".to_string(),
            message_id: "msg-1".to_string(),
            session_id: "s1".to_string(),
            seq: memory_store
                .next_turn_seq("s1")
                .await
                .expect("seq should load"),
            timestamp,
            items: vec![RunItem::UserInput {
                text: "history that should be dropped".to_string(),
                file_ids: Vec::new(),
            }],
            usage: TokenUsage::default(),
            stop_reason: TalosApiStopReason::EndTurn,
        })
        .await
        .expect("append should succeed");
}
