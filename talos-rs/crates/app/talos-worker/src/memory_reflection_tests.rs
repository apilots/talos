use super::*;
use talos_agent_api::{
    AgentEventPayload, AgentMessageBlock, AgentMessageBlockStatus, AgentMessageRole,
    AgentMessageStatus, AgentToolResult, AgentTurnCompleted, AgentTurnRef,
};
use talos_memory::{
    MemoryAgentId, UserWorkspaceMemoryEntry, UserWorkspaceMemoryProducer,
    UserWorkspaceMemorySection,
};
use talos_types::MemoryKind;

#[test]
fn reflection_requires_a_successful_end_turn() {
    let event = AgentEvent {
        identity: AgentTurnRef::new("session-1", "turn-1"),
        sequence: 4,
        occurred_at: 1,
        payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    };
    assert!(reflection_is_eligible(&event));
}

#[test]
fn reflection_parser_accepts_only_noop_upsert_and_remove() {
    assert!(parse_reflection_update(r#"{"operation":"noop"}"#)
        .unwrap()
        .is_none());
    assert!(matches!(
        parse_reflection_update(r#"{"operation":"remove_entry","key":"stale-context"}"#),
        Ok(Some(UserWorkspaceMemoryUpdate::RemoveEntry { key })) if key == "stale-context"
    ));
    assert!(parse_reflection_update(
        r#"{"operation":"create_entry","entry":{"key":"x","section":"key_knowledge","title":"X","kind":"fact","summary":"X"}}"#
    )
    .is_err());
    assert!(
        parse_reflection_update(r#"{"operation":"remove_entry","key":"x","unexpected":true}"#)
            .is_err()
    );
}

#[test]
fn reflection_skips_semantically_unchanged_updates() {
    let entry = UserWorkspaceMemoryEntry::builder()
        .key("stable-preference")
        .section(UserWorkspaceMemorySection::InteractionGuidance)
        .title("Stable preference")
        .kind(MemoryKind::Preference)
        .summary("Keep responses concise.")
        .build()
        .unwrap();
    let document = talos_memory::UserWorkspaceMemoryDocument::template(
        UserWorkspaceMemoryProducer::new(MemoryAgentId::parse("agent-1").unwrap(), 1_784_251_800),
    )
    .unwrap()
    .upsert_entry(entry.clone())
    .unwrap();
    let snapshot = UserWorkspaceMemorySnapshot::Valid {
        raw_content: document.render().unwrap(),
        updated_at: None,
        version_token: "v1".to_string(),
        document,
    };

    assert!(update_is_noop(
        &snapshot,
        &UserWorkspaceMemoryUpdate::UpsertEntry { entry }
    ));
    assert!(update_is_noop(
        &snapshot,
        &UserWorkspaceMemoryUpdate::RemoveEntry {
            key: "unknown".to_string()
        }
    ));
}

fn message(content: AgentMessageBlockContent) -> AgentMessage {
    AgentMessage {
        message_id: "message-1".to_string(),
        turn_id: "turn-1".to_string(),
        role: AgentMessageRole::Tool,
        status: AgentMessageStatus::Completed,
        blocks: vec![AgentMessageBlock {
            block_id: "block-1".to_string(),
            status: AgentMessageBlockStatus::Completed,
            content,
            started_at: 1,
            completed_at: Some(1),
        }],
        created_at: 1,
        completed_at: Some(1),
    }
}

struct ReflectionModel(std::sync::Mutex<Vec<talos_llm_provider::ChatRequest>>);
#[async_trait::async_trait]
impl talos_llm_provider::LlmProvider for ReflectionModel {
    fn name(&self) -> &str {
        "test"
    }
    fn default_model(&self) -> String {
        "model".into()
    }
    async fn chat(
        &self,
        request: talos_llm_provider::ChatRequest,
    ) -> talos_error::TalosResult<talos_llm_provider::ChatResponse> {
        let model = request.model.clone();
        self.0.lock().unwrap().push(request);
        Ok(talos_llm_provider::ChatResponse {
            content: r#"{"operation":"noop"}"#.into(),
            reasoning: Default::default(),
            model,
            usage: talos_llm_provider::Usage {
                prompt_tokens: 2,
                completion_tokens: 1,
                total_tokens: 3,
                ..Default::default()
            },
            finish_reason: Some("stop".into()),
            tool_calls: None,
        })
    }
    async fn stream(
        &self,
        _request: talos_llm_provider::ChatRequest,
    ) -> talos_error::TalosResult<talos_llm_provider::ChatStream> {
        unreachable!("reflection uses chat")
    }
}
struct ReflectionRouter(Arc<ReflectionModel>);
impl talos_llm_provider::LlmProviderRouter for ReflectionRouter {
    fn provider_for(
        &self,
        _model: &talos_types::ModelRef,
    ) -> talos_error::TalosResult<Arc<dyn talos_llm_provider::LlmProvider>> {
        Ok(self.0.clone())
    }
}

#[tokio::test]
async fn reflection_reads_a_large_turn_through_bounded_canonical_windows() {
    reflect_large_local_turn(/*successful_memory_tool*/ false).await;
}

#[tokio::test]
async fn reflection_skips_successful_memory_tools_outside_its_recent_window() {
    reflect_large_local_turn(/*successful_memory_tool*/ true).await;
}

async fn reflect_large_local_turn(successful_memory_tool: bool) {
    let directory = tempfile::tempdir().unwrap();
    let history = Arc::new(
        talos_store::execution::WorkerExecutionStore::open(&directory.path().join("worker.sqlite"))
            .await
            .unwrap(),
    );
    let scope = agent_protocol::AgentSessionContext {
        agent_id: "agent-1".into(),
        session_id: "local-session".into(),
        workspace: talos_config::EffectiveWorkspace {
            workspace_id: "default".into(),
            user_id: "alice".into(),
            root: directory.path().join("workspace"),
        },
    };
    let identity = AgentTurnRef::new(&scope.session_id, "turn-1");
    let mut input = message(AgentMessageBlockContent::UserContent(
        AgentUserContent::Text {
            text: "LOCAL_WORKER_HISTORY_ONLY".into(),
        },
    ));
    input.role = AgentMessageRole::User;
    let mut messages = vec![input.clone()];
    if successful_memory_tool {
        let mut result = message(AgentMessageBlockContent::ToolResult(AgentToolResult {
            metrics: None,
            call_id: "remember-call".into(),
            tool_name: MEMORY_TOOL_PUBLIC_NAME.into(),
            content: Vec::new(),
            is_error: false,
        }));
        result.message_id = "memory-tool-result".into();
        messages.push(result);
    }
    for index in 0..140 {
        let mut progress = message(AgentMessageBlockContent::AssistantText(
            talos_agent_api::AgentAssistantText {
                phase: AgentAssistantTextPhase::Commentary,
                text: "x".repeat(128 * 1024),
            },
        ));
        progress.message_id = format!("progress-{index}");
        progress.role = AgentMessageRole::Agent;
        messages.push(progress);
    }
    let mut final_answer = message(AgentMessageBlockContent::AssistantText(
        talos_agent_api::AgentAssistantText {
            phase: AgentAssistantTextPhase::FinalAnswer,
            text: "LOCAL_WORKER_FINAL_RESPONSE".into(),
        },
    ));
    final_answer.message_id = "final-answer".into();
    final_answer.role = AgentMessageRole::Agent;
    messages.push(final_answer);
    let checkpoint = agent_protocol::AgentRunCheckpoint {
        last_sequence: messages.len() as u64 + 1,
        messages: messages.clone(),
        metrics: Default::default(),
    };
    assert!(serde_json::to_vec(&checkpoint).unwrap().len() > 16 * 1024 * 1024);
    let mut events = vec![AgentEvent {
        identity: identity.clone(),
        sequence: 1,
        occurred_at: 1,
        payload: AgentEventPayload::TurnStarted(talos_agent_api::AgentTurnStarted {}),
    }];
    events.extend(
        messages
            .into_iter()
            .enumerate()
            .map(|(index, message)| AgentEvent {
                identity: identity.clone(),
                sequence: index as u64 + 2,
                occurred_at: 1,
                payload: AgentEventPayload::MessageCompleted(message),
            }),
    );
    history
        .admit(
            &scope,
            &agent_protocol::AgentExecutionCommand::Input {
                params: Box::new(agent_protocol::AgentInputParams {
                    agent_id: scope.agent_id.clone(),
                    identity: identity.clone(),
                    input,
                    command: None,
                }),
            },
            events,
            &checkpoint,
        )
        .await
        .unwrap();
    let bounded = history
        .history_messages(&identity, None, 128, 16 * 1024 * 1024)
        .await
        .unwrap();
    assert!(bounded.has_more);
    assert!(!bounded
        .messages
        .iter()
        .any(|message| message.message_id == "memory-tool-result"));
    let model = talos_types::ModelInfo::from_provider_model(
        "test",
        talos_config::ProviderModelConfig {
            id: "model".into(),
            name: "Model".into(),
            context_window: 32_768,
            reasoning: false,
            reasoning_effort: Default::default(),
            reasoning_efforts: None,
            reasoning_replay: Default::default(),
            request_parameters: Default::default(),
            speed: Default::default(),
            input: Vec::new(),
            cost: Default::default(),
        },
    );
    let context = agent_protocol::AgentContext {
        tree_identity: SessionTreeIdentity::Primary,
        history_cursor: None,
        session: scope,
        turn_id: identity.turn_id.clone(),
        project_id: None,
        working_directory: None,
        environment: None,
        model,
        model_selection: talos_types::ModelSelection::without_reasoning(talos_types::ModelRef {
            provider: "test".into(),
            id: "model".into(),
        }),
        permission_mode: talos_types::UserPermissionMode::FullAccess,
    };
    let provider = Arc::new(ReflectionModel(std::sync::Mutex::new(Vec::new())));
    let mut config = talos_config::Config::default();
    config.agent.inner_model = Some(talos_types::ModelRef {
        provider: "test".into(),
        id: "worker-inner-model".into(),
    });
    config.llm = serde_json::from_value(serde_json::json!({
        "provider":"test", "model":"worker-inner-model", "custom_providers":{
            "test":{"name":"Test", "protocol":"chat", "base_url":"http://127.0.0.1:9",
            "models":[{"id":"worker-inner-model"}]}
        }
    }))
    .unwrap();
    let reflection = UserWorkspaceMemoryReflection {
        config: Arc::new(std::sync::RwLock::new(config)),
        provider: Arc::new(ReflectionRouter(provider.clone())),
        history,
        memory: Arc::new(talos_service::UserWorkspaceMemoryService::new()),
    };
    let event = AgentEvent {
        identity,
        sequence: checkpoint.last_sequence + 1,
        occurred_at: 2,
        payload: AgentEventPayload::TurnCompleted(AgentTurnCompleted {
            stop_reason: AgentStopReason::EndTurn,
        }),
    };
    let usage = reflection.reflect(&context, &event).await;
    let requests = provider.0.lock().unwrap();
    if successful_memory_tool {
        assert!(requests.is_empty());
        assert!(usage.is_empty());
        return;
    }
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].model, "worker-inner-model");
    assert!(requests[0].messages[1]
        .content
        .contains("LOCAL_WORKER_HISTORY_ONLY"));
    assert!(requests[0].messages[1]
        .content
        .contains("LOCAL_WORKER_FINAL_RESPONSE"));
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].usage.total_tokens, 3);
}
