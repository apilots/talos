use super::*;

fn model_call(
    provider: Arc<SummaryProvider>,
) -> impl FnOnce(
    Vec<ChatMessage>,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = TalosResult<ChatResponse>> + Send>,
> {
    move |messages| {
        Box::pin(async move {
            provider
                .chat(
                    ChatRequest::builder()
                        .model("summary-model")
                        .messages(messages)
                        .build(),
                )
                .await
        })
    }
}

#[tokio::test]
async fn dream_memory_processes_each_session_tail_once() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).expect("workspace");
    let memory_store = TestMemoryStore::new();
    memory_store.set_day_entries(vec![talos_memory::DreamMemoryEntry::builder()
        .memory_id(
            talos_memory::MemoryId::parse("mem_0123456789abcdef0123456789abcdef")
                .expect("memory id"),
        )
        .title("Existing preference")
        .body("The user already prefers concise architecture.")
        .occurred_at(Utc::now().timestamp())
        .kind(talos_types::MemoryKind::Preference)
        .importance(talos_types::MemoryImportance::High)
        .provenance(talos_memory::DreamMemoryProvenance::UserAuthored)
        .build()
        .expect("Dream Memory entry")]);
    seed_recent_user_message(&memory_store).await;
    let provider = Arc::new(SummaryProvider::new(vec![r#"{
            "mode":"maintenance",
            "memory_candidates":[{
                "title":"Concise architecture",
                "body":"The user prefers concise architecture.",
                "kind":"preference",
                "importance":"high"
            }]
        }"#
    .to_string()]));
    let config = AgentKernelConfig {
        workspace_dir: workspace.to_string_lossy().to_string(),
        ..AgentKernelConfig::default()
    };
    let scope = MemoryScope {
        workspace_id: "default".to_string(),
        user_id: "user-1".to_string(),
        workspace_root: workspace.clone(),
    };

    let first = crate::form_dream_session_memory(
        MemoryFormationParams {
            session_id: "s1",
            producer_agent_id: "agent-1",
            dream_memory_store: &memory_store,
            memory_scope: &scope,
            session_artifact_store: &memory_store,
            session_prompt_context_source: &memory_store,
            memory_progress_store: &memory_store,
            kernel_config: &config,
        },
        model_call(provider.clone()),
    )
    .await
    .expect("Dream Memory should succeed");
    let second = crate::form_dream_session_memory(
        MemoryFormationParams {
            session_id: "s1",
            producer_agent_id: "agent-1",
            dream_memory_store: &memory_store,
            memory_scope: &scope,
            session_artifact_store: &memory_store,
            session_prompt_context_source: &memory_store,
            memory_progress_store: &memory_store,
            kernel_config: &config,
        },
        model_call(provider.clone()),
    )
    .await
    .expect("Dream Memory retry should succeed");

    assert_eq!(first.covers_until_seq, Some(1));
    assert_eq!(second.covers_until_seq, None);
    assert_eq!(memory_store.writes().len(), 1);
    {
        let requests = provider
            .requests
            .lock()
            .expect("summary provider requests mutex should lock");
        assert_eq!(
            requests[0].messages[0].content,
            super::super::dream_memory_prompt::DREAM_MEMORY_FORMATION_PROMPT
        );
        assert!(requests[0].messages[1]
            .content
            .contains("Mode: maintenance"));
        assert!(requests[0].messages[1]
            .content
            .contains("Existing preference"));
    }
    assert_eq!(
        memory_store
            .processed_until_event_seq("s1")
            .await
            .expect("load session cursor"),
        1
    );
    std::fs::remove_dir_all(workspace).expect("remove workspace");
}

#[tokio::test]
async fn dream_memory_does_not_advance_cursor_when_write_fails() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).expect("workspace");
    let artifacts = TestMemoryStore::new();
    seed_recent_user_message(&artifacts).await;
    let provider = Arc::new(SummaryProvider::new(vec![r#"{
            "mode":"maintenance",
            "memory_candidates":[]
        }"#
    .to_string()]));
    let config = AgentKernelConfig {
        workspace_dir: workspace.to_string_lossy().to_string(),
        ..AgentKernelConfig::default()
    };
    let scope = MemoryScope {
        workspace_id: "default".to_string(),
        user_id: "user-1".to_string(),
        workspace_root: workspace.clone(),
    };

    let error = crate::form_dream_session_memory(
        MemoryFormationParams {
            session_id: "s1",
            producer_agent_id: "agent-1",
            dream_memory_store: &FailingMemoryWriter,
            memory_scope: &scope,
            session_artifact_store: &artifacts,
            session_prompt_context_source: &artifacts,
            memory_progress_store: &artifacts,
            kernel_config: &config,
        },
        model_call(provider),
    )
    .await
    .expect_err("failed write should fail Dream Memory");

    assert!(error.is_internal());
    assert_eq!(
        artifacts
            .processed_until_event_seq("s1")
            .await
            .expect("load session cursor"),
        0
    );
    std::fs::remove_dir_all(workspace).expect("remove workspace");
}

#[tokio::test]
async fn dream_memory_bounds_long_messages_and_still_advances() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).expect("workspace");
    let memory_store = TestMemoryStore::new();
    memory_store.append_prompt_turn(
        "turn-1",
        1,
        vec![(SessionPromptMessageRole::User, "x".repeat(100_000))],
    );
    let provider = Arc::new(SummaryProvider::new(vec![
        r#"{"mode":"maintenance","memory_candidates":[]}"#.to_string(),
    ]));
    let config = AgentKernelConfig {
        workspace_dir: workspace.to_string_lossy().to_string(),
        ..AgentKernelConfig::default()
    };
    let scope = MemoryScope {
        workspace_id: "default".to_string(),
        user_id: "user-1".to_string(),
        workspace_root: workspace.clone(),
    };

    let report = crate::form_dream_session_memory(
        MemoryFormationParams {
            session_id: "s1",
            producer_agent_id: "agent-1",
            dream_memory_store: &memory_store,
            memory_scope: &scope,
            session_artifact_store: &memory_store,
            session_prompt_context_source: &memory_store,
            memory_progress_store: &memory_store,
            kernel_config: &config,
        },
        model_call(provider.clone()),
    )
    .await
    .expect("bounded Dream Memory should succeed");

    let requests = provider
        .requests
        .lock()
        .expect("summary provider requests mutex should lock");
    assert!(requests[0].messages[1].content.chars().count() <= 4_100);
    assert_eq!(report.covers_until_seq, Some(1));
    std::fs::remove_dir_all(workspace).expect("remove workspace");
}
