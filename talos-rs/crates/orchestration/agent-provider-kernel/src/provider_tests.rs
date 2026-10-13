use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use talos_agent::{
    create_agent_harness, AgentContext, AgentFuture, AgentInputParams, AgentResult, AgentRunParams,
    AgentRunSettlement, AgentSessionContext, AgentUserContentKind, AttachmentReader,
    ResolvedAttachment, SessionHistoryReadParams, SessionHistoryReader, TurnFileScope,
};
use talos_agent_api::{
    AgentAssistantText, AgentAssistantTextPhase, AgentEventPayload, AgentMessage,
    AgentMessageBlock, AgentMessageBlockContent, AgentMessageBlockStatus, AgentMessageRole,
    AgentMessageStatus, AgentTurnRef, AgentUserContent, SessionHistoryTurn,
};
use talos_agent_kernel::{
    EventAuthor, ExecutionBranch, KernelMemoryProgressStore, KernelSessionArtifactStore,
    KernelTranscriptEvent, KernelTranscriptPayload,
};
use talos_config::{AgentKernelConfig, EffectiveWorkspace, ProviderModelConfig};
use talos_error::TalosResult;
use talos_llm_provider::{
    ChatRequest, ChatResponse, ChatStream, LlmProvider, LlmProviderRouter, ModelBlockCompleted,
    ModelBlockDelta, ModelBlockStarted, ModelBlockStream, ModelBlockStreamEvent, ModelOutputPhase,
    ReasoningOutput, Usage,
};
use talos_memory::{
    DreamMemoryDate, DreamMemoryDayRevision, DreamMemoryDaySnapshot, DreamMemoryFormationStore,
    DreamMemoryWriteRequest, MemoryAgentId, MemoryScope, UserWorkspaceMemoryDocument,
    UserWorkspaceMemoryEntry, UserWorkspaceMemoryEntryQuery, UserWorkspaceMemoryProducer,
    UserWorkspaceMemoryReader, UserWorkspaceMemorySnapshot,
};
use talos_skills::SkillRoots;
use talos_tools::ToolRegistry;
use talos_types::{ModelInfo, ModelRef, ModelSelection, UserPermissionMode};

use super::*;
use crate::FileKernelSessionArtifactStore;

struct StaticRouter;
#[derive(Default)]
struct TextProvider {
    stream_calls: AtomicU64,
}
struct NoopDreamMemoryStore;
struct MissingUserWorkspaceMemory;
struct TestSkillRootsResolver;
struct EmptyHistory;

#[derive(Default)]
struct MemoryProgress {
    processed_until_event_seq: AtomicU64,
}

#[async_trait::async_trait]
impl KernelMemoryProgressStore for MemoryProgress {
    async fn processed_until_event_seq(&self, _session_id: &str) -> TalosResult<u64> {
        Ok(self.processed_until_event_seq.load(Ordering::SeqCst))
    }

    async fn advance_processed_until_event_seq(
        &self,
        _session_id: &str,
        expected_until_seq: u64,
        next_until_seq: u64,
    ) -> TalosResult<()> {
        self.processed_until_event_seq
            .compare_exchange(
                expected_until_seq,
                next_until_seq,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .map(|_| ())
            .map_err(|current| {
                talos_error::TalosError::new(
                    talos_error::ErrorCode::Conflict,
                    format!(
                        "memory cursor conflict: expected {expected_until_seq}, current {current}"
                    ),
                )
            })
    }
}

impl SessionHistoryReader for EmptyHistory {
    fn read(
        &self,
        _params: SessionHistoryReadParams,
    ) -> AgentFuture<'_, AgentResult<Vec<SessionHistoryTurn>>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

#[async_trait::async_trait]
impl LlmProvider for TextProvider {
    fn name(&self) -> &str {
        "test"
    }

    fn default_model(&self) -> String {
        "model".to_string()
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: "provider answer".to_string(),
            reasoning: ReasoningOutput::default(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        let (_sender, receiver) = tokio::sync::mpsc::channel(1);
        Ok(ChatStream { rx: receiver })
    }

    async fn stream_model_blocks(&self, _request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let (sender, receiver) = tokio::sync::mpsc::channel(4);
        let stream_call = self.stream_calls.fetch_add(1, Ordering::SeqCst);
        tokio::spawn(async move {
            sender
                .send(Ok(ModelBlockStreamEvent::BlockStarted(
                    ModelBlockStarted::assistant_text(None, ModelOutputPhase::Unknown),
                )))
                .await
                .unwrap();
            let (chunk, chunk_count) = if stream_call == 0 {
                ("x", 1_024)
            } else {
                ("provider answer", 1)
            };
            for _ in 0..chunk_count {
                sender
                    .send(Ok(ModelBlockStreamEvent::BlockDelta {
                        model_item_id: "assistant_text".to_string(),
                        delta: ModelBlockDelta::AssistantText {
                            delta: chunk.to_string(),
                        },
                    }))
                    .await
                    .unwrap();
            }
            sender
                .send(Ok(ModelBlockStreamEvent::BlockCompleted(
                    ModelBlockCompleted {
                        model_item_id: "assistant_text".to_string(),
                    },
                )))
                .await
                .unwrap();
            sender
                .send(Ok(ModelBlockStreamEvent::Done {
                    finish_reason: Some("stop".to_string()),
                    usage: Some(Usage::default()),
                }))
                .await
                .unwrap();
        });
        Ok(ModelBlockStream::new(receiver))
    }
}

impl LlmProviderRouter for StaticRouter {
    fn provider_for(&self, _model_ref: &ModelRef) -> TalosResult<Arc<dyn LlmProvider>> {
        Ok(Arc::new(TextProvider::default()))
    }
}

impl DreamMemoryFormationStore for NoopDreamMemoryStore {
    fn read_day<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        date: DreamMemoryDate,
    ) -> Pin<Box<dyn Future<Output = TalosResult<DreamMemoryDaySnapshot>> + Send + 'a>> {
        Box::pin(async move {
            Ok(DreamMemoryDaySnapshot::new(
                date,
                DreamMemoryDayRevision::Missing,
                Vec::new(),
            ))
        })
    }

    fn write<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        _request: DreamMemoryWriteRequest,
    ) -> Pin<Box<dyn Future<Output = TalosResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

impl UserWorkspaceMemoryReader for MissingUserWorkspaceMemory {
    fn read<'a>(
        &'a self,
        _scope: &'a MemoryScope,
    ) -> Pin<Box<dyn Future<Output = TalosResult<UserWorkspaceMemorySnapshot>> + Send + 'a>> {
        Box::pin(async {
            Ok(UserWorkspaceMemorySnapshot::Missing {
                template: UserWorkspaceMemoryDocument::template(UserWorkspaceMemoryProducer::new(
                    MemoryAgentId::parse("agent-1").unwrap(),
                    1_784_251_800,
                ))
                .unwrap(),
            })
        })
    }

    fn get_entry<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        _key: &'a str,
    ) -> Pin<Box<dyn Future<Output = TalosResult<Option<UserWorkspaceMemoryEntry>>> + Send + 'a>>
    {
        Box::pin(async { Ok(None) })
    }

    fn list_entries<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        _query: UserWorkspaceMemoryEntryQuery,
    ) -> Pin<Box<dyn Future<Output = TalosResult<Vec<UserWorkspaceMemoryEntry>>> + Send + 'a>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

impl SkillRootsResolver for TestSkillRootsResolver {
    fn skill_roots(&self, kernel_config: &AgentKernelConfig) -> TalosResult<SkillRoots> {
        Ok(SkillRoots {
            roots: Vec::new(),
            state_path: Path::new(&kernel_config.workspace_dir).join("skills-state.json"),
            max_skill_md_bytes: kernel_config.skills.max_skill_md_bytes,
            max_available_summary_bytes: kernel_config.skills.max_available_summary_bytes,
        })
    }
}

#[tokio::test]
async fn kernel_provider_keeps_streaming_while_live_input_waits_for_a_safe_boundary() {
    let temp_dir = tempfile::tempdir().unwrap();
    let provider = provider(temp_dir.path());
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let harness = create_agent_harness("agent-1".to_string(), provider);
    let context = context(identity, temp_dir.path());
    let execution = harness
        .start(AgentRunParams {
            context,
            input: user_message(),
            checkpoint: None,
            continuation: None,
        })
        .await
        .unwrap();
    execution.admission.permit.consume().await.unwrap();
    let mut progress = execution.events;
    loop {
        let event = progress.recv().await.expect("Provider progress must start");
        if matches!(event.payload, AgentEventPayload::MessageBlockDelta(_)) {
            break;
        }
    }
    let progress_drain = tokio::spawn(async move { while progress.recv().await.is_some() {} });
    let admission = harness
        .submit_input(AgentInputParams {
            agent_id: "agent-1".to_string(),
            identity: AgentTurnRef::new("session-1", "turn-1"),
            input: live_user_message(),
        })
        .await
        .unwrap();
    let (input_result, settlement) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(admission.permit.consume(), execution.settlement.recv())
    })
    .await
    .expect("live input and Turn must not deadlock at the event buffer boundary");
    input_result.unwrap();
    let AgentRunSettlement::Finished { messages, .. } = settlement.unwrap() else {
        panic!("Kernel Provider Turn must finish");
    };
    progress_drain.abort();
    let final_text = messages
        .iter()
        .flat_map(|message| &message.blocks)
        .filter_map(|block| match &block.content {
            AgentMessageBlockContent::AssistantText(AgentAssistantText {
                phase: AgentAssistantTextPhase::FinalAnswer,
                text,
            }) => Some(text.as_str()),
            _ => None,
        })
        .next_back();
    assert_eq!(final_text, Some("provider answer"));
    let steer_position = messages
        .iter()
        .position(|message| message.message_id == live_user_message().message_id)
        .unwrap();
    let answer_position = messages.iter().rposition(|message| message.blocks.iter().any(|block| matches!(
        &block.content,
        AgentMessageBlockContent::AssistantText(text) if text.phase == AgentAssistantTextPhase::FinalAnswer
    ))).unwrap();
    assert!(
        answer_position > steer_position,
        "the response to live input must follow it in canonical history"
    );
}

fn live_user_message() -> AgentMessage {
    AgentMessage {
        message_id: "message-2".to_string(),
        turn_id: "turn-1".to_string(),
        role: AgentMessageRole::User,
        status: AgentMessageStatus::Completed,
        blocks: vec![AgentMessageBlock {
            block_id: "block-2".to_string(),
            status: AgentMessageBlockStatus::Completed,
            content: AgentMessageBlockContent::UserContent(AgentUserContent::Text {
                text: "steer".to_string(),
            }),
            started_at: 2,
            completed_at: Some(2),
        }],
        created_at: 2,
        completed_at: Some(2),
    }
}

#[tokio::test]
async fn kernel_provider_clear_session_deletes_only_private_artifacts() {
    let temp_dir = tempfile::tempdir().unwrap();
    let artifact_store = Arc::new(FileKernelSessionArtifactStore);
    let mut transcript = KernelTranscriptEvent::new(
        "session-1",
        "turn-1",
        ExecutionBranch::ModelThinking,
        EventAuthor::Agent,
        KernelTranscriptPayload::SessionState {
            update: talos_agent_kernel::SessionStateUpdate::Compaction(
                talos_agent_kernel::SessionCompactionState::normal(),
            ),
        },
    );
    transcript.id = "transcript-1".to_string();
    transcript.timestamp = Utc::now();
    artifact_store
        .append_transcript_event(temp_dir.path(), &transcript)
        .await
        .unwrap();
    let harness = create_agent_harness(
        "agent-1".to_string(),
        provider_with_store(temp_dir.path(), artifact_store.clone()),
    );
    harness
        .clear_session(
            AgentSessionContext {
                agent_id: "agent-1".to_string(),
                session_id: "session-1".to_string(),
                workspace: workspace(temp_dir.path()),
            },
            Default::default(),
        )
        .await
        .unwrap();

    assert_eq!(
        artifact_store
            .load_transcript_events(temp_dir.path(), "session-1", None, None)
            .await
            .unwrap(),
        Vec::new()
    );
}

#[test]
fn kernel_provider_rejects_capabilities_the_adapter_cannot_execute() {
    let temp_dir = tempfile::tempdir().unwrap();
    let mut params = provider_params(temp_dir.path(), Arc::new(FileKernelSessionArtifactStore));
    params.rich_input.push(AgentUserContentKind::Audio);

    let Err(error) = KernelAgentProvider::new(params) else {
        panic!("unsupported capability must be rejected");
    };

    assert_eq!(error.error_code(), talos_error::ErrorCode::InvalidRequest);
}

#[test]
fn effective_kernel_config_comes_from_the_current_agent_context() {
    let temp_dir = tempfile::tempdir().unwrap();
    let provider = provider(temp_dir.path());
    let context = context(AgentTurnRef::new("session-1", "turn-1"), temp_dir.path());

    let config = provider.effective_kernel_config(&context);

    assert_eq!(
        (
            config.workspace_dir,
            config.tools.sandbox_policy.user_permission_mode,
        ),
        (
            temp_dir.path().to_string_lossy().into_owned(),
            talos_config::ToolUserPermissionModeConfig::FullAccess,
        )
    );
}

fn provider(workspace_root: &Path) -> KernelAgentProvider {
    provider_with_store(workspace_root, Arc::new(FileKernelSessionArtifactStore))
}

fn provider_with_store(
    workspace_root: &Path,
    session_artifact_store: Arc<dyn KernelSessionArtifactStore>,
) -> KernelAgentProvider {
    KernelAgentProvider::new(provider_params(workspace_root, session_artifact_store)).unwrap()
}

fn provider_params(
    workspace_root: &Path,
    session_artifact_store: Arc<dyn KernelSessionArtifactStore>,
) -> KernelAgentProviderParams {
    KernelAgentProviderParams {
        agent_id: "agent-1".to_string(),
        name: "Test Agent".to_string(),
        description: "Kernel Provider contract test".to_string(),
        llm_provider_router: Arc::new(StaticRouter),
        tool_registry: Arc::new(ToolRegistry::new()),
        kernel_config: AgentKernelConfig {
            workspace_dir: workspace_root.display().to_string(),
            ..AgentKernelConfig::default()
        },
        skill_roots_resolver: Arc::new(TestSkillRootsResolver),
        session_artifact_store,
        session_history_reader: Arc::new(EmptyHistory),
        memory_progress_store: Arc::new(MemoryProgress::default()),
        dream_memory_store: Arc::new(NoopDreamMemoryStore),
        user_workspace_memory_reader: Arc::new(MissingUserWorkspaceMemory),
        attachment_reader: Arc::new(EmptyAttachmentReader),
        rich_input: rich_input(),
    }
}

struct EmptyAttachmentReader;

impl AttachmentReader for EmptyAttachmentReader {
    fn read<'a>(
        &'a self,
        _scope: &'a TurnFileScope,
        _file_ids: &'a [String],
    ) -> Pin<Box<dyn Future<Output = TalosResult<Vec<ResolvedAttachment>>> + Send + 'a>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

fn rich_input() -> Vec<AgentUserContentKind> {
    vec![
        AgentUserContentKind::SkillSelection,
        AgentUserContentKind::File,
    ]
}

fn context(identity: AgentTurnRef, workspace_root: &Path) -> AgentContext {
    AgentContext {
        history_cursor: None,
        session: AgentSessionContext {
            agent_id: "agent-1".to_string(),
            session_id: identity.session_id,
            workspace: workspace(workspace_root),
        },
        turn_id: identity.turn_id,
        project_id: None,
        working_directory: Some(workspace_root.to_path_buf()),
        environment: None,
        model: ModelInfo::from_provider_model(
            "test".to_string(),
            ProviderModelConfig {
                id: "model".to_string(),
                name: "Model".to_string(),
                context_window: 32_768,
                reasoning: false,
                reasoning_effort: Default::default(),
                speed: Default::default(),
                input: Vec::new(),
                cost: Default::default(),
            },
        ),
        model_selection: ModelSelection::without_reasoning(ModelRef {
            provider: "test".to_string(),
            id: "model".to_string(),
        }),
        permission_mode: UserPermissionMode::FullAccess,
    }
}

fn workspace(root: &Path) -> EffectiveWorkspace {
    EffectiveWorkspace {
        workspace_id: "workspace-1".to_string(),
        user_id: "user-1".to_string(),
        root: root.to_path_buf(),
    }
}

fn user_message() -> AgentMessage {
    AgentMessage {
        message_id: "message-1".to_string(),
        turn_id: "turn-1".to_string(),
        role: AgentMessageRole::User,
        status: AgentMessageStatus::Completed,
        blocks: vec![AgentMessageBlock {
            block_id: "block-1".to_string(),
            status: AgentMessageBlockStatus::Completed,
            content: AgentMessageBlockContent::UserContent(AgentUserContent::Text {
                text: "hello".to_string(),
            }),
            started_at: 1,
            completed_at: Some(1),
        }],
        created_at: 1,
        completed_at: Some(1),
    }
}
