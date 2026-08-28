use super::*;

#[tokio::test]
async fn compaction_persists_candidates_and_summary_from_one_response() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).expect("workspace");
    let memory_store = TestMemoryStore::new(workspace.clone());
    seed_recent_user_message(&memory_store).await;

    let config = AgentKernelConfig {
        workspace_dir: workspace.to_string_lossy().to_string(),
        ..AgentKernelConfig::default()
    };
    let scope = MemoryScope {
        agent_id: "agent-1".to_string(),
        user_id: "user-1".to_string(),
        workspace_root: workspace.clone(),
    };
    let session_artifact_store = talos_store::session::FileSessionArtifactStore::default();
    let session_store = talos_store::session::MockSessionStore::new();
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
            selected_model: "summary-model",
            llm_provider: provider.clone(),
            dream_memory_store: &memory_store,
            memory_scope: &scope,
            session_artifact_store: &session_artifact_store,
            session_store: &session_store,
            kernel_config: &config,
        },
        MemoryCompactionPolicy::for_compaction(),
    )
    .await
    .expect("compaction should succeed");

    assert_eq!(report.request_count, 1);
    assert_eq!(report.covers_from_seq, Some(1));
    assert_eq!(report.covers_until_seq, Some(1));
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
        session_store
            .get_session_record("s1")
            .await
            .expect("load session cursor")
            .expect("session record")
            .memory_processed_until_seq,
        1
    );
    assert_eq!(
        session_artifact_store
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
    let artifacts = TestMemoryStore::new(workspace.clone());
    seed_recent_user_message(&artifacts).await;
    let config = AgentKernelConfig {
        workspace_dir: workspace.to_string_lossy().to_string(),
        ..AgentKernelConfig::default()
    };
    let session_artifact_store = talos_store::session::FileSessionArtifactStore::default();
    let session_store = talos_store::session::MockSessionStore::new();

    let error = crate::compact_session_memory(
        MemoryFormationParams {
            session_id: "s1",
            selected_model: "summary-model",
            llm_provider: Arc::new(SummaryProvider::new(vec![r#"{
                    "mode":"compaction",
                    "title":"Architecture preference",
                    "memory_candidates":[]
                }"#
            .to_string()])),
            dream_memory_store: &FailingMemoryWriter,
            memory_scope: &MemoryScope {
                agent_id: "agent-1".to_string(),
                user_id: "user-1".to_string(),
                workspace_root: workspace.clone(),
            },
            session_artifact_store: &session_artifact_store,
            session_store: &session_store,
            kernel_config: &config,
        },
        MemoryCompactionPolicy::for_compaction(),
    )
    .await
    .expect_err("failed memory write should fail compaction");

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
    assert!(session_artifact_store
        .load_summaries(&workspace, "s1", 10)
        .await
        .expect("load summaries")
        .is_empty());
    std::fs::remove_dir_all(workspace).expect("remove workspace");
}
