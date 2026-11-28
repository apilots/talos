use super::*;
use talos_agent_kernel::{compact_session_memory, MemoryCompactionPolicy, MemoryFormationParams};
use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider};
use talos_memory::{
    DreamMemoryDate, DreamMemoryDayRevision, DreamMemoryDaySnapshot, DreamMemoryFormationStore,
    DreamMemoryWriteRequest, MemoryScope,
};

struct NoopDreamStore;
impl DreamMemoryFormationStore for NoopDreamStore {
    fn read_day<'a>(
        &'a self,
        _scope: &'a MemoryScope,
        date: DreamMemoryDate,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = talos_error::TalosResult<DreamMemoryDaySnapshot>>
                + Send
                + 'a,
        >,
    > {
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
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = talos_error::TalosResult<()>> + Send + 'a>,
    > {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Default)]
struct CompactionModel(std::sync::Mutex<Vec<ChatRequest>>);
#[async_trait::async_trait]
impl LlmProvider for CompactionModel {
    fn name(&self) -> &str {
        "bounded-compaction"
    }
    fn default_model(&self) -> String {
        "bounded-compaction".into()
    }
    async fn chat(&self, request: ChatRequest) -> talos_error::TalosResult<ChatResponse> {
        let model = request.model.clone();
        self.0.lock().unwrap().push(request);
        Ok(ChatResponse {
            content: r#"{"mode":"compaction","title":"Bounded history","memory_candidates":[]}"#
                .into(),
            reasoning: Default::default(),
            model,
            usage: Default::default(),
            finish_reason: Some("stop".into()),
            tool_calls: None,
        })
    }
    async fn stream(&self, _request: ChatRequest) -> talos_error::TalosResult<ChatStream> {
        unreachable!("compaction uses chat")
    }
}

#[tokio::test]
async fn compaction_budget_reloads_the_real_source_before_future_tool_outputs() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(
        WorkerExecutionStore::open(&directory.path().join("budget.sqlite"))
            .await
            .unwrap(),
    );
    let session = scope();
    admit_turn(&store, &session, 0, 600, false).await;
    let identity = AgentTurnRef::new(&session.session_id, "turn-0");
    let mut checkpoint = store.checkpoint(&identity).await.unwrap().unwrap();
    let mut extra = checkpoint.messages[1].clone();
    extra.message_id = "future-tool-result".into();
    extra.blocks[0].block_id = "future-result".into();
    checkpoint.messages.push(extra.clone());
    checkpoint.last_sequence = 5;
    store
        .append(
            &session,
            vec![
                AgentEvent {
                    identity: identity.clone(),
                    sequence: 4,
                    occurred_at: 1,
                    payload: AgentEventPayload::MessageCompleted(extra),
                },
                AgentEvent {
                    identity,
                    sequence: 5,
                    occurred_at: 1,
                    payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
                        stop_reason: AgentStopReason::EndTurn,
                    }),
                },
            ],
            &checkpoint,
            Some(AgentExecutionSettlement::Finished {
                memory_usage: vec![],
                metrics: checkpoint.metrics.clone(),
            }),
        )
        .await
        .unwrap();
    let history = WorkerExecutionHistory(store.clone());
    let artifacts = Arc::new(talos_agent_provider_kernel::FileKernelSessionArtifactStore);
    let source = talos_agent_provider_kernel::KernelSessionPromptContextSource::new(
        Arc::new(history.clone()),
        artifacts.clone(),
        directory.path().to_owned(),
        session.session_id.clone(),
    );
    let mut config = talos_config::AgentKernelConfig {
        workspace_dir: directory.path().to_string_lossy().into(),
        ..Default::default()
    };
    // Keep both existing protected outputs: the read window must move, rather
    // than disabling an enabled class or discarding an unchanged source cursor.
    config.compaction.tool_output_keep_last = 2;
    let memory_scope = MemoryScope {
        workspace_id: "default".into(),
        user_id: "alice".into(),
        workspace_root: directory.path().to_owned(),
    };
    let model = Arc::new(CompactionModel::default());
    let mut covered = Vec::new();
    for _ in 0..3 {
        let result = compact_session_memory(
            MemoryFormationParams {
                session_id: &session.session_id,
                producer_agent_id: "agent-1",
                dream_memory_store: &NoopDreamStore,
                memory_scope: &memory_scope,
                session_artifact_store: artifacts.as_ref(),
                session_prompt_context_source: &source,
                memory_progress_store: &history,
                kernel_config: &config,
            },
            "bounded-compaction",
            model.clone(),
            MemoryCompactionPolicy::for_compaction().with_compaction_input_budget_tokens(Some(256)),
        )
        .await
        .unwrap();
        covered.push(result.covers_until_seq);
    }
    assert_eq!(covered, vec![Some(2), Some(3), Some(5)]);
    assert_eq!(store.memory_progress(&session.session_id).await.unwrap(), 5);
    let requests = model.0.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(!requests[0].messages[1]
        .content
        .contains("Tool read_file result"));
    assert!(requests[1].messages[1]
        .content
        .contains("Tool read_file result"));
}
