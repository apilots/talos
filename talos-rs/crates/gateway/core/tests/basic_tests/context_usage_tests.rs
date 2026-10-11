use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use talos_agent::{
    create_agent_harness, AgentHarness, AgentUserContentKind, AttachmentReader, ResolvedAttachment,
    TurnFileScope,
};
use talos_agent_kernel::KernelMemoryProgressStore;
use talos_agent_provider_kernel::{
    empty_skill_roots_resolver, FileKernelSessionArtifactStore, KernelAgentProvider,
    KernelAgentProviderParams,
};
use talos_coordinator::CoordinatorImpl;
use talos_error::TalosResult;
use talos_llm_provider::{
    ChatRequest, ChatResponse, ChatStream, LlmProvider, LlmProviderRouter, ModelBlockCompleted,
    ModelBlockDelta, ModelBlockStarted, ModelBlockStream, ModelBlockStreamEvent, ModelOutputPhase,
    ReasoningOutput, Usage,
};
use talos_memory::{DreamMemoryFormationStore, MemoryScope};
use talos_store::session::SessionStore;
use talos_tools::{Tool, ToolContext, ToolOutputPayload, ToolRegistry};
use talos_tools_policy::ToolPolicy;
use talos_types::{ModelRef, RecoverTimelineRequest, SessionActiveTurnRequest, SessionGetRequest};
use tokio::sync::mpsc;

const AGENT_ID: &str = "context-usage-agent";
const SESSION_ID: &str = "context-usage-session";

struct StaticRouter;

struct EmptyAttachmentReader;

impl AttachmentReader for EmptyAttachmentReader {
    fn read<'a>(
        &'a self,
        _scope: &'a TurnFileScope,
        _file_ids: &'a [String],
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = TalosResult<Vec<ResolvedAttachment>>> + Send + 'a>,
    > {
        Box::pin(async { Ok(Vec::new()) })
    }
}

struct SessionMemoryProgress {
    sessions: Arc<dyn SessionStore>,
}

#[async_trait]
impl KernelMemoryProgressStore for SessionMemoryProgress {
    async fn processed_until_event_seq(&self, session_id: &str) -> TalosResult<u64> {
        self.sessions
            .get_session_record(session_id)
            .await?
            .map(|record| record.memory_processed_until_event_seq)
            .ok_or_else(|| {
                talos_error::TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    session_id.to_string(),
                )
            })
    }

    async fn advance_processed_until_event_seq(
        &self,
        session_id: &str,
        expected_until_seq: u64,
        next_until_seq: u64,
    ) -> TalosResult<()> {
        self.sessions
            .advance_memory_processed_until_event_seq(
                session_id,
                expected_until_seq,
                next_until_seq,
            )
            .await
    }
}

#[async_trait]
impl talos_router::Router for StaticRouter {
    async fn route(&self, _channel: &str, _user_id: &str) -> TalosResult<Option<String>> {
        Ok(Some(AGENT_ID.to_string()))
    }

    async fn add_binding(&self, _binding: talos_router::Binding) -> TalosResult<()> {
        Ok(())
    }

    async fn remove_binding(&self, _id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn list_bindings(&self) -> TalosResult<Vec<talos_router::Binding>> {
        Ok(Vec::new())
    }
}

struct StaticProviderRouter {
    provider: Arc<dyn LlmProvider>,
}

impl LlmProviderRouter for StaticProviderRouter {
    fn provider_for(&self, _model_ref: &ModelRef) -> TalosResult<Arc<dyn LlmProvider>> {
        Ok(self.provider.clone())
    }
}

#[derive(Default)]
struct TwoRoundProvider {
    round: AtomicUsize,
}

#[async_trait]
impl LlmProvider for TwoRoundProvider {
    fn name(&self) -> &str {
        "context-usage-test"
    }

    fn default_model(&self) -> String {
        "claude-3-5-sonnet".to_string()
    }

    async fn chat(&self, request: ChatRequest) -> TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: "Context usage test".to_string(),
            reasoning: ReasoningOutput::default(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
        unreachable!("streamed integration test should use model block streaming")
    }

    async fn stream_model_blocks(&self, _request: ChatRequest) -> TalosResult<ModelBlockStream> {
        let round = self.round.fetch_add(1, Ordering::SeqCst);
        let events = if round == 0 {
            vec![
                ModelBlockStreamEvent::BlockStarted(ModelBlockStarted::tool_call(
                    0,
                    Some("call-1".to_string()),
                    Some("echo_usage".to_string()),
                )),
                ModelBlockStreamEvent::BlockDelta {
                    model_item_id: "tool_call_0".to_string(),
                    delta: ModelBlockDelta::ToolArguments {
                        delta: "{\"message\":\"ping\"}".to_string(),
                    },
                },
                ModelBlockStreamEvent::BlockCompleted(ModelBlockCompleted {
                    model_item_id: "tool_call_0".to_string(),
                }),
                ModelBlockStreamEvent::Done {
                    finish_reason: Some("tool_calls".to_string()),
                    usage: Some(Usage {
                        prompt_tokens: 50,
                        completion_tokens: 20,
                        total_tokens: 70,
                        ..Usage::default()
                    }),
                },
            ]
        } else {
            vec![
                ModelBlockStreamEvent::BlockStarted(ModelBlockStarted::assistant_text(
                    None,
                    ModelOutputPhase::FinalAnswer,
                )),
                ModelBlockStreamEvent::BlockDelta {
                    model_item_id: "assistant_text".to_string(),
                    delta: ModelBlockDelta::AssistantText {
                        delta: "done".to_string(),
                    },
                },
                ModelBlockStreamEvent::BlockCompleted(ModelBlockCompleted {
                    model_item_id: "assistant_text".to_string(),
                }),
                ModelBlockStreamEvent::Done {
                    finish_reason: Some("stop".to_string()),
                    usage: Some(Usage {
                        prompt_tokens: 90,
                        completion_tokens: 20,
                        total_tokens: 110,
                        ..Usage::default()
                    }),
                },
            ]
        };
        let (tx, rx) = mpsc::channel(events.len());
        tokio::spawn(async move {
            for event in events {
                tx.send(Ok(event)).await.expect("model event receiver open");
            }
        });
        Ok(ModelBlockStream::new(rx))
    }
}

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo_usage"
    }

    fn description(&self) -> &str {
        "Return a deterministic integration-test result"
    }

    fn parameters_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": { "message": { "type": "string" } }
        }))
    }

    async fn execute_payload(
        &self,
        _input: serde_json::Value,
        _context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        Ok(ToolOutputPayload::success_text("echo:ping".to_string()))
    }
}

struct NoopMemoryWriter;

impl DreamMemoryFormationStore for NoopMemoryWriter {
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
        Box::pin(async { Ok(()) })
    }
}

fn build_agent(config: &TalosConfig, store: Arc<DatabaseSessionStore>) -> Arc<dyn AgentHarness> {
    let registry = Arc::new(ToolRegistry::new_with_policy(ToolPolicy::allow_list(vec![
        "echo_usage",
    ])));
    registry.register(Arc::new(EchoTool));
    let provider_router: Arc<dyn LlmProviderRouter> = Arc::new(StaticProviderRouter {
        provider: Arc::new(TwoRoundProvider::default()),
    });
    let user_workspace_memory = Arc::new(talos_service::UserWorkspaceMemoryService::new());
    let session_history_reader = Arc::new(talos_service::SessionHistoryService::new(Arc::new(
        talos_service::TimelineEventService::new(store.clone()),
    )));
    let provider = KernelAgentProvider::new(KernelAgentProviderParams {
        agent_id: AGENT_ID.to_string(),
        name: "Context Usage Agent".to_string(),
        description: "Context usage integration test agent".to_string(),
        llm_provider_router: provider_router,
        tool_registry: registry,
        kernel_config: config.agent.kernel.clone(),
        skill_roots_resolver: empty_skill_roots_resolver(),
        session_artifact_store: Arc::new(FileKernelSessionArtifactStore),
        session_history_reader,
        memory_progress_store: Arc::new(SessionMemoryProgress { sessions: store }),
        dream_memory_store: Arc::new(NoopMemoryWriter),
        user_workspace_memory_reader: user_workspace_memory,
        attachment_reader: Arc::new(EmptyAttachmentReader),
        rich_input: vec![AgentUserContentKind::File],
    })
    .expect("integration Provider should build");
    create_agent_harness(AGENT_ID.to_string(), provider)
}

#[tokio::test]
async fn multi_round_usage_persists_the_last_request_context_through_session_get() {
    let workspace_root = tempfile::tempdir().expect("test Workspace should be created");
    let mut config = test_config();
    config.workspaces.definitions = vec![talos_config::WorkspaceDefinition::new(
        "default",
        workspace_root.path(),
    )];
    config.agent.kernel.prompt.bootstrap.enabled = false;
    config.agent.kernel.prompt.window.compaction_buffer_tokens = 8;
    config.agent.kernel.prompt.window.reserved_output_tokens =
        talos_config::ReservedOutputPolicy::Fixed { tokens: 32 };
    let (storage, store) = test_session_storage().await;
    let agent = build_agent(&config, store.clone());
    let node_store = Arc::new(talos_store::node::DatabaseAgentNodeStore::new(
        storage.pool().clone(),
    ));
    talos_service::AgentNodeService::new(node_store.clone())
        .register_embedded(true)
        .await
        .unwrap();
    let coordinator = Arc::new(CoordinatorImpl::new(
        Arc::new(StaticRouter),
        store.clone(),
        node_store,
    ));
    coordinator
        .register_environment(
            "embedded".into(),
            Arc::new(talos_service::WorkerEnvironmentService::new(
                Arc::new(std::sync::RwLock::new(config.clone())),
                Some(std::env::temp_dir()),
            )),
        )
        .await;
    coordinator
        .register_agent("embedded".into(), AGENT_ID.to_string(), agent)
        .await
        .expect("integration agent should register");
    let gateway = test_gateway_with_coordinator(
        config,
        storage,
        store,
        coordinator as Arc<dyn AgentCoordinator>,
    )
    .await;
    let actor = Actor::regular("user-1");

    gateway
        .start_turn(
            &actor,
            super::request_source(),
            StartTurnRequest {
                node_id: None,
                session_id: SESSION_ID.to_string(),
                input: AgentUserInput {
                    content: vec![AgentUserContent::Text {
                        text: "Use the tool, then answer.".to_string(),
                    }],
                },
                model_selection: None,
                user_permission_mode: None,
                project_id: None,
                current_directory: None,
                client_request_id: None,
            },
            None,
        )
        .await
        .expect("turn should start");

    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let active = gateway
                .get_active_turn(
                    &actor,
                    SessionActiveTurnRequest {
                        session_id: SESSION_ID.to_string(),
                    },
                )
                .await
                .expect("active turn should be readable");
            if active.active_turn.is_none() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("turn should complete");
    let recovery = gateway
        .recover_timeline(
            &actor,
            RecoverTimelineRequest {
                session_id: SESSION_ID.to_string(),
                after_event_id: None,
                replay_limit: Some(100),
            },
        )
        .await
        .expect("terminal Timeline should be readable");
    assert!(
        matches!(
            recovery.replay.events.last(),
            Some(talos_agent_api::TimelineEvent {
                event: talos_agent_api::TimelineEventPayload::Agent(talos_agent_api::AgentEvent {
                    payload: talos_agent_api::AgentEventPayload::TurnCompleted(_),
                    ..
                }),
                ..
            })
        ),
        "unexpected terminal trace: {:?}",
        recovery.replay.events
    );

    let session = gateway
        .get_session(
            &actor,
            SessionGetRequest {
                session_id: SESSION_ID.to_string(),
            },
        )
        .await
        .expect("completed session should be readable")
        .session;
    assert_eq!(session.usage.tokens.total_tokens, 180);
    let context = session
        .usage
        .context
        .expect("completed session should expose context usage");
    assert_eq!(
        Some(context.clone()),
        talos_types::ContextUsage::new(110, 32_768, 32_728, context.measured_at)
    );
}
