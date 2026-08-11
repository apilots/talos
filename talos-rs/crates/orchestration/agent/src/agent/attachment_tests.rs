use super::*;
use talos_agent_kernel::{AttachmentReader, ResolvedAttachment, TurnFileScope};

#[derive(Default)]
struct RecordingFailingAttachmentReader {
    calls: Mutex<Vec<(TurnFileScope, Vec<String>)>>,
}

impl AttachmentReader for RecordingFailingAttachmentReader {
    fn read<'a>(
        &'a self,
        scope: &'a TurnFileScope,
        file_ids: &'a [String],
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = talos_error::TalosResult<Vec<ResolvedAttachment>>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            self.calls
                .lock()
                .expect("attachment reader calls mutex should lock")
                .push((scope.clone(), file_ids.to_vec()));
            Err(talos_error::TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                "one or more files are unavailable".to_string(),
            ))
        })
    }
}

#[tokio::test]
async fn run_uses_session_provider_for_llm_routing() {
    let seen_provider = Arc::new(Mutex::new(None));
    let seen_model = Arc::new(Mutex::new(None));
    let primary = Arc::new(NamedRecordingProvider {
        provider_name: "primary",
        seen_provider: seen_provider.clone(),
        seen_model: seen_model.clone(),
    });
    let secondary = Arc::new(NamedRecordingProvider {
        provider_name: "secondary",
        seen_provider: seen_provider.clone(),
        seen_model: seen_model.clone(),
    });
    let agent = test_agent_harness(
        "test_agent".to_string(),
        "Test Agent".to_string(),
        "An agent for testing".to_string(),
        Arc::new(RecordingLlmProviderRouter { primary, secondary }),
        ModelSelection::without_reasoning(ModelRef {
            provider: "openai".to_string(),
            id: "primary-model".to_string(),
        }),
        Arc::new(ToolRegistry::new()),
        AgentKernelConfig::default(),
    )
    .expect("agent harness should build");

    let mut session = build_test_session("session-provider-routing", "workspace-1");
    session.conf.model_selection = Some(ModelSelection::without_reasoning(ModelRef {
        provider: "deepseek".to_string(),
        id: "secondary-model".to_string(),
    }));

    let result = agent
        .run(agent_run_params(
            session,
            "turn-provider-routing",
            vec![TalosInputItem::Text {
                text: "hello".to_string(),
            }],
        ))
        .await
        .expect("run should succeed");

    assert_eq!(result.stop_reason, TalosStopReason::EndTurn);
    assert_eq!(
        seen_provider
            .lock()
            .expect("provider lock poisoned")
            .as_deref(),
        Some("secondary")
    );
    assert_eq!(
        seen_model.lock().expect("model lock poisoned").as_deref(),
        Some("secondary-model")
    );
}

#[tokio::test]
async fn run_resolves_file_ids_with_session_owner_and_project_before_model_call() {
    let seen_provider = Arc::new(Mutex::new(None));
    let seen_model = Arc::new(Mutex::new(None));
    let provider = Arc::new(NamedRecordingProvider {
        provider_name: "recording",
        seen_provider: seen_provider.clone(),
        seen_model: seen_model.clone(),
    });
    let reader = Arc::new(RecordingFailingAttachmentReader::default());
    let agent = AgentHarnessImpl::new(AgentHarnessParams {
        id: "test_agent".to_string(),
        name: "Test Agent".to_string(),
        description: "An agent for testing".to_string(),
        llm_provider_router: static_router(provider),
        model_selection: model_selection("agent-model"),
        tool_registry: Arc::new(ToolRegistry::new()),
        kernel_config: AgentKernelConfig::default(),
        skill_roots_resolver: crate::empty_skill_roots_resolver(),
        session_artifact_store: Arc::new(FileSessionArtifactStore::default()),
        session_store: Arc::new(talos_store::session::MockSessionStore::new()),
        memory_writer: Arc::new(TestMemoryWriter),
        attachment_reader: reader.clone(),
    })
    .expect("agent harness should build");
    let mut session = build_test_session("session-files", "workspace-1");
    session.project_id = Some("project-7".to_string());

    let error = agent
        .run(agent_run_params(
            session,
            "turn-files",
            vec![TalosInputItem::Files {
                file_ids: vec!["file-2".to_string(), "file-1".to_string()],
            }],
        ))
        .await
        .expect_err("reader authorization failure must reject the complete turn");

    assert_eq!(
        reader
            .calls
            .lock()
            .expect("attachment reader calls mutex should lock")
            .as_slice(),
        [(
            TurnFileScope {
                owner_user_id: "talos-test-local-user".to_string(),
                project_id: Some("project-7".to_string()),
            },
            vec!["file-2".to_string(), "file-1".to_string()],
        )]
    );
    assert_eq!(error.kind, AgentErrorKind::RunNotFound);
    assert!(!error.retryable);
    assert_eq!(
        seen_provider
            .lock()
            .expect("provider capture mutex should lock")
            .as_ref(),
        None
    );
    assert_eq!(
        seen_model
            .lock()
            .expect("model capture mutex should lock")
            .as_ref(),
        None
    );
}
