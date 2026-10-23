use super::*;
use crate::{
    ExecutionBranch, InvocationSession, KernelMemoryProgressStore, KernelSessionArtifactStore,
    KernelSessionSummary, KernelTranscriptEvent, KernelTranscriptPayload, KernelTranscriptService,
    SessionPromptContext, SessionPromptContextFuture, SessionPromptContextSource,
    SessionPromptEntryHeader, SessionPromptMessage, SessionPromptSourceTurn, SessionPromptSummary,
    SessionState,
};

struct TestMemoryStore {
    session_id: Mutex<String>,
    writes: Mutex<Vec<MemoryWriteBatch>>,
    day_entries: Mutex<Vec<talos_memory::DreamMemoryEntry>>,
    prompt_context: Mutex<SessionPromptContext>,
    summaries: Mutex<Vec<KernelSessionSummary>>,
    transcript: Mutex<Vec<KernelTranscriptEvent>>,
    memory_cursor: std::sync::atomic::AtomicU64,
}

impl TestMemoryStore {
    fn new() -> Self {
        Self {
            session_id: Mutex::new("s1".to_string()),
            writes: Mutex::new(Vec::new()),
            day_entries: Mutex::new(Vec::new()),
            prompt_context: Mutex::new(SessionPromptContext::default()),
            summaries: Mutex::new(Vec::new()),
            transcript: Mutex::new(Vec::new()),
            memory_cursor: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn set_session_id(&self, session_id: impl Into<String>) {
        *self
            .session_id
            .lock()
            .expect("test Session id mutex should lock") = session_id.into();
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

    fn append_prompt_turn(
        &self,
        turn_id: impl Into<String>,
        completed_event_seq: i64,
        messages: Vec<(SessionPromptMessageRole, String)>,
    ) {
        let turn_id = turn_id.into();
        let mut context = self
            .prompt_context
            .lock()
            .expect("test prompt context mutex should lock");
        context.source_turns.push(SessionPromptSourceTurn {
            turn_id: turn_id.clone(),
            completed_event_seq,
            completed_at: completed_event_seq,
        });
        context
            .recent_messages
            .extend(
                messages
                    .into_iter()
                    .enumerate()
                    .map(|(index, (role, text))| SessionPromptMessage {
                        header: SessionPromptEntryHeader {
                            id: format!("{turn_id}:message:{index}"),
                            turn_id: turn_id.clone(),
                            completed_at: completed_event_seq,
                        },
                        role,
                        text,
                    }),
            );
    }
}

#[async_trait]
impl KernelTranscriptService for TestMemoryStore {
    async fn append_event(
        &self,
        session: &mut InvocationSession,
        event: KernelTranscriptEvent,
    ) -> TalosResult<KernelTranscriptEvent> {
        if let KernelTranscriptPayload::SessionState { update } = &event.payload {
            session.state.apply(update);
        }
        self.transcript
            .lock()
            .expect("test transcript mutex should lock")
            .push(event.clone());
        session.events.push(event.clone());
        Ok(event)
    }

    async fn load_session(
        &self,
        session_id: &str,
        invocation_id: Option<&str>,
        branch: Option<ExecutionBranch>,
    ) -> TalosResult<InvocationSession> {
        let events = self
            .transcript
            .lock()
            .expect("test transcript mutex should lock")
            .iter()
            .filter(|event| {
                event.session_id == session_id
                    && invocation_id.is_none_or(|id| event.invocation_id == id)
                    && branch.is_none_or(|value| event.branch == value)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut state = SessionState::default();
        for event in &events {
            if let KernelTranscriptPayload::SessionState { update } = &event.payload {
                state.apply(update);
            }
        }
        Ok(InvocationSession {
            session_id: session_id.to_string(),
            events,
            state,
        })
    }
}

impl SessionPromptContextSource for TestMemoryStore {
    fn load(&self, config: SessionPromptContextConfig) -> SessionPromptContextFuture<'_> {
        Box::pin(async move {
            let mut context = self
                .prompt_context
                .lock()
                .expect("test prompt context mutex should lock")
                .clone();
            if let Some(cursor) = config.compact_cursor {
                let uncovered_turns = context
                    .source_turns
                    .iter()
                    .filter(|turn| {
                        u64::try_from(turn.completed_event_seq)
                            .is_ok_and(|sequence| sequence > cursor.covers_until_seq)
                    })
                    .map(|turn| turn.turn_id.clone())
                    .collect::<std::collections::HashSet<_>>();
                context
                    .source_turns
                    .retain(|turn| uncovered_turns.contains(&turn.turn_id));
                context
                    .recent_messages
                    .retain(|message| uncovered_turns.contains(&message.header.turn_id));
            }
            if context.recent_messages.len() > config.recent_messages_limit {
                match config.recent_window {
                    SessionPromptRecentWindow::Latest => {
                        context.recent_messages = context.recent_messages.split_off(
                            context.recent_messages.len() - config.recent_messages_limit,
                        );
                    }
                    SessionPromptRecentWindow::OldestUncovered => {
                        context
                            .recent_messages
                            .truncate(config.recent_messages_limit);
                    }
                }
            }
            context.summaries = if config.include_summary {
                let session_id = self
                    .session_id
                    .lock()
                    .expect("test Session id mutex should lock")
                    .clone();
                self.load_test_summaries(&session_id, config.summary_recent_count)
                    .into_iter()
                    .map(|summary| SessionPromptSummary {
                        id: summary.header.id,
                        covers_until_seq: summary.covers_until_seq,
                        source_turn_count: summary.source_turn_count,
                        content: if config.summary_max_chars == 0 {
                            summary.content
                        } else {
                            summary
                                .content
                                .chars()
                                .take(config.summary_max_chars)
                                .collect()
                        },
                    })
                    .collect()
            } else {
                Vec::new()
            };
            Ok(context)
        })
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
        Box::pin(async { Err(TalosError::internal("memory write failed")) })
    }
}

impl TestMemoryStore {
    fn load_test_summaries(&self, session_id: &str, limit: usize) -> Vec<KernelSessionSummary> {
        let summaries = self
            .summaries
            .lock()
            .expect("test summaries mutex should lock");
        let mut matching = summaries
            .iter()
            .filter(|summary| summary.header.session_id == session_id)
            .cloned()
            .collect::<Vec<_>>();
        if matching.len() > limit {
            matching = matching.split_off(matching.len() - limit);
        }
        matching
    }
}

#[async_trait]
impl KernelSessionArtifactStore for TestMemoryStore {
    async fn append_transcript_event(
        &self,
        _workspace_root: &Path,
        event: &KernelTranscriptEvent,
    ) -> TalosResult<()> {
        self.transcript
            .lock()
            .expect("test transcript mutex should lock")
            .push(event.clone());
        Ok(())
    }

    async fn load_transcript_events(
        &self,
        _workspace_root: &Path,
        session_id: &str,
        invocation_id: Option<&str>,
        branch: Option<ExecutionBranch>,
    ) -> TalosResult<Vec<KernelTranscriptEvent>> {
        Ok(self
            .transcript
            .lock()
            .expect("test transcript mutex should lock")
            .iter()
            .filter(|event| {
                event.session_id == session_id
                    && invocation_id.is_none_or(|value| event.invocation_id == value)
                    && branch.is_none_or(|value| event.branch == value)
            })
            .cloned()
            .collect())
    }

    async fn save_summary(
        &self,
        _workspace_root: &Path,
        summary: &KernelSessionSummary,
    ) -> TalosResult<()> {
        self.summaries
            .lock()
            .expect("test summaries mutex should lock")
            .push(summary.clone());
        Ok(())
    }

    async fn load_summaries(
        &self,
        _workspace_root: &Path,
        session_id: &str,
        limit: usize,
    ) -> TalosResult<Vec<KernelSessionSummary>> {
        Ok(self.load_test_summaries(session_id, limit))
    }

    async fn session_size_bytes(
        &self,
        _workspace_root: &Path,
        _session_id: &str,
    ) -> TalosResult<u64> {
        Ok(0)
    }

    async fn reset_session(&self, _workspace_root: &Path, session_id: &str) -> TalosResult<()> {
        self.summaries
            .lock()
            .expect("test summaries mutex should lock")
            .retain(|summary| summary.header.session_id != session_id);
        self.transcript
            .lock()
            .expect("test transcript mutex should lock")
            .retain(|event| event.session_id != session_id);
        Ok(())
    }
}

#[async_trait]
impl KernelMemoryProgressStore for TestMemoryStore {
    async fn processed_until_event_seq(&self, _session_id: &str) -> TalosResult<u64> {
        Ok(self.memory_cursor.load(Ordering::SeqCst))
    }

    async fn advance_processed_until_event_seq(
        &self,
        _session_id: &str,
        expected_until_seq: u64,
        next_until_seq: u64,
    ) -> TalosResult<()> {
        match self.memory_cursor.compare_exchange(
            expected_until_seq,
            next_until_seq,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => Ok(()),
            Err(current) if current >= next_until_seq => Ok(()),
            Err(current) => Err(TalosError::new(
                talos_error::ErrorCode::Conflict,
                format!("memory cursor conflict: expected {expected_until_seq}, current {current}"),
            )),
        }
    }
}
use crate::{AgentKernelEventPayload, ToolExecutionMode, ToolProgressKind, TurnPhase};
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
    dream_memory_store: Arc<TestMemoryStore>,
    kernel_config: &AgentKernelConfig,
    skill_options: GeneralTurnSkillOptions,
) -> TalosResult<GeneralTurnResult> {
    dream_memory_store.set_session_id(ctx.session_id.clone());
    ctx.session_prompt_context_source = dream_memory_store.clone();
    ctx.session_artifact_store = dream_memory_store.clone();
    ctx.memory_progress_store = dream_memory_store.clone();
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
    dream_memory_store: Arc<TestMemoryStore>,
    kernel_config: &AgentKernelConfig,
    skill_options: Option<GeneralTurnSkillOptions>,
) -> TalosResult<PreparedGeneralTurn> {
    dream_memory_store.set_session_id(ctx.session_id.clone());
    ctx.session_prompt_context_source = dream_memory_store.clone();
    ctx.session_artifact_store = dream_memory_store.clone();
    ctx.memory_progress_store = dream_memory_store.clone();
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
    stream_tx: Option<mpsc::Sender<crate::AgentKernelEvent>>,
) -> InvocationContext {
    build_ctx_with_stream_for_session("s1", stream_tx)
}

fn build_ctx_with_stream_for_session(
    session_id: impl Into<String>,
    stream_tx: Option<mpsc::Sender<crate::AgentKernelEvent>>,
) -> InvocationContext {
    let session_id = session_id.into();
    let kernel_state = Arc::new(TestMemoryStore::new());
    InvocationContext {
        session_id: session_id.clone(),
        user_id: "u1".to_string(),
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
                reasoning_replay: Default::default(),
                request_parameters: Default::default(),
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
        session: crate::InvocationSession {
            session_id: session_id.clone(),
            ..crate::InvocationSession::default()
        },
        session_environment: None,
        transcript_service: kernel_state.clone(),
        session_artifact_store: kernel_state.clone(),
        session_prompt_context_source: kernel_state.clone(),
        memory_progress_store: kernel_state,
        stream_tx,
    }
}

async fn seed_recent_user_message(memory_store: &TestMemoryStore) {
    memory_store.append_prompt_turn(
        "turn-1",
        1,
        vec![(
            SessionPromptMessageRole::User,
            "history that should be dropped".to_string(),
        )],
    );
}
