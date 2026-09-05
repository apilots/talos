use super::*;

#[tokio::test]
async fn compaction_persists_candidates_and_summary_from_one_response() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).expect("workspace");
    let memory_store = TestMemoryStore::new();
    for (turn_id, completed_event_seq, text) in [
        ("turn-1", 5, "first canonical Turn"),
        ("turn-2", 12, "second canonical Turn"),
    ] {
        memory_store.append_prompt_turn(
            turn_id,
            completed_event_seq,
            vec![(SessionPromptMessageRole::User, text.to_string())],
        );
    }

    let config = AgentKernelConfig {
        workspace_dir: workspace.to_string_lossy().to_string(),
        ..AgentKernelConfig::default()
    };
    let scope = MemoryScope {
        workspace_id: "default".to_string(),
        user_id: "user-1".to_string(),
        workspace_root: workspace.clone(),
    };
    let provider = Arc::new(SummaryProvider::new(vec![r#"{
            "mode":"compaction",
            "title":"Architecture preference",
            "preferences":["Prefer concise architecture"],
            "memory_candidates":[{
                "title":"Concise architecture",
                "body":"The user prefers concise architecture.",
                "kind":"preference",
                "importance":"high"
            }]
        }"#
    .to_string()]));

    let report = crate::general_turn::memory_summary::compact_session_memory(
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
        "summary-model",
        provider.clone(),
        MemoryCompactionPolicy::for_compaction(),
    )
    .await
    .expect("compaction should succeed");

    assert_eq!(report.request_count, 1);
    assert_eq!(report.covers_from_seq, Some(5));
    assert_eq!(report.covers_until_seq, Some(12));
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
        assert!(requests[0].messages[1].content.contains("Mode: compaction"));
    }
    assert_eq!(
        memory_store
            .processed_until_event_seq("s1")
            .await
            .expect("load session cursor"),
        12
    );
    assert_eq!(
        memory_store
            .load_summaries(&workspace, "s1", 10)
            .await
            .expect("load summaries")
            .len(),
        1
    );
    std::fs::remove_dir_all(workspace).expect("remove workspace");
}

#[tokio::test]
async fn compaction_does_not_advance_cursor_or_summary_when_memory_write_fails() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).expect("workspace");
    let artifacts = TestMemoryStore::new();
    seed_recent_user_message(&artifacts).await;
    let config = AgentKernelConfig {
        workspace_dir: workspace.to_string_lossy().to_string(),
        ..AgentKernelConfig::default()
    };

    let error = crate::compact_session_memory(
        MemoryFormationParams {
            session_id: "s1",
            producer_agent_id: "agent-1",
            dream_memory_store: &FailingMemoryWriter,
            memory_scope: &MemoryScope {
                workspace_id: "default".to_string(),
                user_id: "user-1".to_string(),
                workspace_root: workspace.clone(),
            },
            session_artifact_store: &artifacts,
            session_prompt_context_source: &artifacts,
            memory_progress_store: &artifacts,
            kernel_config: &config,
        },
        "summary-model",
        Arc::new(SummaryProvider::new(vec![r#"{
                "mode":"compaction",
                "title":"Architecture preference",
                "memory_candidates":[]
            }"#
        .to_string()])),
        MemoryCompactionPolicy::for_compaction(),
    )
    .await
    .expect_err("failed memory write should fail compaction");

    assert!(error.is_internal());
    assert_eq!(
        artifacts
            .processed_until_event_seq("s1")
            .await
            .expect("load session cursor"),
        0
    );
    assert!(artifacts
        .load_summaries(&workspace, "s1", 10)
        .await
        .expect("load summaries")
        .is_empty());
    std::fs::remove_dir_all(workspace).expect("remove workspace");
}
