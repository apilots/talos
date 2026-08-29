use super::*;

#[tokio::test]
async fn dream_memory_processes_each_session_tail_once() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).expect("workspace");
    let memory_store = TestMemoryStore::new(workspace.clone());
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
    let session_artifact_store = talos_store::session::FileSessionArtifactStore::default();
    let session_store = talos_store::session::MockSessionStore::new();
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

    let first = crate::form_dream_session_memory(MemoryFormationParams {
        session_id: "s1",
        producer_agent_id: "agent-1",
        selected_model: "summary-model",
        llm_provider: provider.clone(),
        dream_memory_store: &memory_store,
        memory_scope: &scope,
        session_artifact_store: &session_artifact_store,
        session_store: &session_store,
        kernel_config: &config,
    })
    .await
    .expect("Dream Memory should succeed");
    let second = crate::form_dream_session_memory(MemoryFormationParams {
        session_id: "s1",
        producer_agent_id: "agent-1",
        selected_model: "summary-model",
        llm_provider: provider.clone(),
        dream_memory_store: &memory_store,
        memory_scope: &scope,
        session_artifact_store: &session_artifact_store,
        session_store: &session_store,
        kernel_config: &config,
    })
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
        session_store
            .get_session_record("s1")
            .await
            .expect("load session cursor")
            .expect("session record")
            .memory_processed_until_seq,
        1
    );
    std::fs::remove_dir_all(workspace).expect("remove workspace");
}

#[tokio::test]
async fn dream_memory_does_not_advance_cursor_when_write_fails() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).expect("workspace");
    let artifacts = TestMemoryStore::new(workspace.clone());
    seed_recent_user_message(&artifacts).await;
    let session_artifact_store = talos_store::session::FileSessionArtifactStore::default();
    let session_store = talos_store::session::MockSessionStore::new();
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

    let error = crate::form_dream_session_memory(MemoryFormationParams {
        session_id: "s1",
        producer_agent_id: "agent-1",
        selected_model: "summary-model",
        llm_provider: provider,
        dream_memory_store: &FailingMemoryWriter,
        memory_scope: &scope,
        session_artifact_store: &session_artifact_store,
        session_store: &session_store,
        kernel_config: &config,
    })
    .await
    .expect_err("failed write should fail Dream Memory");

    assert!(error.is_internal());
    assert_eq!(
        session_store
            .get_session_record("s1")
            .await
            .expect("load session cursor")
            .expect("session record")
            .memory_processed_until_seq,
        0
    );
    std::fs::remove_dir_all(workspace).expect("remove workspace");
}

#[tokio::test]
async fn dream_memory_bounds_long_messages_and_still_advances() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).expect("workspace");
    let memory_store = TestMemoryStore::new(workspace.clone());
    memory_store
        .append_turn(&TalosTurn {
            turn_id: "turn-1".to_string(),
            message_id: "msg-1".to_string(),
            session_id: "s1".to_string(),
            seq: 1,
            timestamp: Utc::now(),
            items: vec![RunItem::UserInput {
                text: "x".repeat(100_000),
                file_ids: Vec::new(),
            }],
            usage: TokenUsage::default(),
            stop_reason: TalosApiStopReason::EndTurn,
        })
        .await
        .expect("append long turn");
    let session_artifact_store = talos_store::session::FileSessionArtifactStore::default();
    let session_store = talos_store::session::MockSessionStore::new();
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

    let report = crate::form_dream_session_memory(MemoryFormationParams {
        session_id: "s1",
        producer_agent_id: "agent-1",
        selected_model: "summary-model",
        llm_provider: provider.clone(),
        dream_memory_store: &memory_store,
        memory_scope: &scope,
        session_artifact_store: &session_artifact_store,
        session_store: &session_store,
        kernel_config: &config,
    })
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
