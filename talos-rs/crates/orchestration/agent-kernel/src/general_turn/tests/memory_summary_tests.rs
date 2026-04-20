use super::*;
use crate::general_turn::memory_summary::build_sanitized_memory_decision_input;

#[test]
fn test_build_sanitized_memory_decision_input_does_not_duplicate_facts() {
    let context = talos_memory::SessionPromptContext {
        summaries: Vec::new(),
        recent_messages: vec![talos_memory::TalosMessage {
            header: talos_memory::RecordHeader {
                id: "msg-1".to_string(),
                session_id: "s1".to_string(),
                turn_id: Some("turn-1".to_string()),
                timestamp: Utc::now(),
            },
            role: TalosMessageRole::User,
            text: "Analyze the project architecture from code.".to_string(),
        }],
    };
    let summary = SessionMemorySummary {
        title: "Architecture findings".to_string(),
        facts: vec!["Coordinator wires router and agents.".to_string()],
        preferences: vec!["Prefer code evidence over docs.".to_string()],
        open_tasks: vec!["Write final architecture summary.".to_string()],
        risks: vec!["Docs may describe target architecture.".to_string()],
    };

    let input = build_sanitized_memory_decision_input(&context, &summary);
    assert_eq!(
        input.verified_outcomes,
        vec!["Pending task: Write final architecture summary.".to_string()]
    );
    assert_eq!(
        input.stable_facts_discovered,
        vec!["Coordinator wires router and agents.".to_string()]
    );
}

#[tokio::test]
async fn test_flush_session_memory_rejects_blank_summary_title() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = MemoryStore::new(workspace_dir.clone());
    seed_recent_user_message(&memory_store).await;

    let mut config = AgentKernelConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    flush_session_memory(
        "s1",
        "summary-model",
        Arc::new(SummaryProvider::new(vec![
            "{\"title\":\"   \",\"facts\":[\"remember tea\"]}".to_string(),
        ])),
        &memory_store,
        &config,
        MemoryFlushPolicy::new(false, false, true),
    )
    .await
    .expect("blank summary title should skip without error");
    assert!(
        !workspace_dir.join("memory").exists(),
        "blank summary title should not persist workspace memory"
    );
    assert!(
        memory_store
            .load_summaries("s1", 1)
            .await
            .expect("summary load should succeed")
            .is_empty(),
        "blank summary title should not persist session summary"
    );
}

#[tokio::test]
async fn test_flush_session_memory_returns_early_when_threshold_is_zero() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = MemoryStore::new(workspace_dir.clone());
    seed_recent_user_message(&memory_store).await;

    let mut config = AgentKernelConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.memory.compaction_threshold_bytes = 0;

    flush_session_memory(
        "s1",
        "summary-model",
        Arc::new(SummaryProvider::new(vec![
            "{\"title\":\"Durable Memory\",\"facts\":[\"remember tea\"]}".to_string(),
        ])),
        &memory_store,
        &config,
        MemoryFlushPolicy::new(true, true, true),
    )
    .await
    .expect("threshold zero should skip without error");

    assert!(
        !workspace_dir.join("memory").exists(),
        "threshold zero should skip workspace writes"
    );
    assert!(
        memory_store
            .load_summaries("s1", 1)
            .await
            .expect("summary load should succeed")
            .is_empty(),
        "threshold zero should skip session summary writes"
    );
}

#[tokio::test]
async fn test_flush_session_memory_returns_early_when_transcript_is_blank() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = MemoryStore::new(workspace_dir.clone());
    let timestamp = Utc::now();
    memory_store
        .append_turn(&TalosTurn {
            turn_id: "turn-blank".to_string(),
            session_id: "s1".to_string(),
            seq: memory_store
                .next_turn_seq("s1")
                .await
                .expect("seq should load"),
            timestamp,
            items: vec![TalosOutputItem::UserInput {
                text: "   ".to_string(),
            }],
            usage: TalosUsage::default(),
            stop_reason: TalosApiStopReason::EndTurn,
        })
        .await
        .expect("append should succeed");

    let mut config = AgentKernelConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    flush_session_memory(
        "s1",
        "summary-model",
        Arc::new(SummaryProvider::new(vec![
            "{\"title\":\"Durable Memory\",\"facts\":[\"remember tea\"]}".to_string(),
        ])),
        &memory_store,
        &config,
        MemoryFlushPolicy::new(false, true, true),
    )
    .await
    .expect("blank transcript should skip without error");

    assert!(
        !workspace_dir.join("memory").exists(),
        "blank transcript should not persist workspace writes"
    );
    assert!(
        memory_store
            .load_summaries("s1", 1)
            .await
            .expect("summary load should succeed")
            .is_empty(),
        "blank transcript should not persist session summary"
    );
}

#[tokio::test]
async fn test_flush_session_memory_persists_session_only_summary_without_workspace_append() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = MemoryStore::new(workspace_dir.clone());
    seed_recent_user_message(&memory_store).await;

    let mut config = AgentKernelConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    flush_session_memory(
        "s1",
        "summary-model",
        Arc::new(SummaryProvider::new(vec![
            "{\"title\":\"Short Session\",\"facts\":[\"checked crate roots\"],\"open_tasks\":[\"write architecture summary\"]}".to_string(),
            "{\"decision\":\"session_only\",\"rationale\":\"Useful near-term recap but not durable.\"}".to_string(),
        ])),
        &memory_store,
        &config,
        MemoryFlushPolicy::new(false, true, true),
    )
    .await
    .expect("session_only decision should succeed");

    assert_eq!(
        memory_store
            .load_summaries("s1", 10)
            .await
            .expect("summary load should succeed")
            .len(),
        1
    );
    assert!(
        !workspace_dir.join("memory").exists(),
        "session_only should not append workspace memory"
    );
}

#[tokio::test]
async fn test_flush_session_memory_persists_durable_summary_and_workspace_memory() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = MemoryStore::new(workspace_dir.clone());
    seed_recent_user_message(&memory_store).await;

    let mut config = AgentKernelConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    flush_session_memory(
        "s1",
        "summary-model",
        Arc::new(SummaryProvider::new(vec![
            "{\"title\":\"Durable Preference\",\"facts\":[\"User prefers code-first architecture analysis\"],\"preferences\":[\"Prefer code evidence over docs\"]}".to_string(),
            "{\"decision\":\"durable\",\"rationale\":\"Contains stable preference and durable working rule.\"}".to_string(),
        ])),
        &memory_store,
        &config,
        MemoryFlushPolicy::new(false, true, true),
    )
    .await
    .expect("durable decision should succeed");

    assert_eq!(
        memory_store
            .load_summaries("s1", 10)
            .await
            .expect("summary load should succeed")
            .len(),
        1
    );
    assert!(
        workspace_dir.join("memory").exists(),
        "durable decision should append workspace memory"
    );
}

#[tokio::test]
async fn test_flush_session_memory_close_flush_appends_workspace_for_session_only_decision() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = MemoryStore::new(workspace_dir.clone());
    seed_recent_user_message(&memory_store).await;

    let mut config = AgentKernelConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    flush_session_memory(
        "s1",
        "summary-model",
        Arc::new(SummaryProvider::new(vec![
            "{\"title\":\"Close Flush Note\",\"facts\":[\"Session should be archived on close\"],\"open_tasks\":[\"none\"]}".to_string(),
            "{\"decision\":\"session_only\",\"rationale\":\"Useful session recap but not durable.\"}".to_string(),
        ])),
        &memory_store,
        &config,
        MemoryFlushPolicy::for_session_close(true),
    )
    .await
    .expect("close flush should allow session_only workspace append");

    assert!(
        workspace_dir.join("memory").exists(),
        "close flush should append workspace memory for session_only"
    );
    assert!(
        memory_store
            .load_summaries("s1", 10)
            .await
            .expect("summary load should succeed")
            .is_empty(),
        "close flush path should not persist session summary when disabled"
    );
}

#[tokio::test]
async fn test_flush_session_memory_close_flush_uses_title_as_minimal_workspace_body() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = MemoryStore::new(workspace_dir.clone());
    seed_recent_user_message(&memory_store).await;

    let mut config = AgentKernelConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    flush_session_memory(
        "s1",
        "summary-model",
        Arc::new(SummaryProvider::new(vec![
            "{\"title\":\"Close flush reminder\"}".to_string(),
        ])),
        &memory_store,
        &config,
        MemoryFlushPolicy::for_session_close(true),
    )
    .await
    .expect("close flush should append even when summary body is empty");

    let memory_dir = workspace_dir.join("memory");
    let mut day_files = std::fs::read_dir(&memory_dir)
        .expect("workspace memory dir should exist")
        .filter_map(|entry| entry.ok().map(|item| item.path()))
        .collect::<Vec<_>>();
    day_files.sort();
    let day_file = day_files
        .into_iter()
        .last()
        .expect("workspace memory file should exist");
    let content = std::fs::read_to_string(day_file).expect("workspace memory file should read");
    assert!(content.contains("session: s1"));
    assert!(content.contains("Close flush reminder"));
}

#[tokio::test]
async fn test_flush_session_memory_retries_invalid_summary_response_before_success() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = MemoryStore::new(workspace_dir.clone());
    seed_recent_user_message(&memory_store).await;

    let mut config = AgentKernelConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    flush_session_memory(
        "s1",
        "summary-model",
        Arc::new(SummaryProvider::new(vec![
            "summary text without valid json".to_string(),
            "{\"title\":\"Retried summary\",\"facts\":[\"remember retry\"]}".to_string(),
            "{\"decision\":\"skip\",\"rationale\":\"low value\"}".to_string(),
        ])),
        &memory_store,
        &config,
        MemoryFlushPolicy::new(false, true, true),
    )
    .await
    .expect("invalid summary json should retry and then succeed");

    assert!(
        memory_store
            .load_summaries("s1", 10)
            .await
            .expect("summary load should succeed")
            .is_empty(),
        "skip decision should avoid summary persistence after retry"
    );
    assert!(
        !workspace_dir.join("memory").exists(),
        "skip decision should avoid workspace append after retry"
    );
}

#[tokio::test]
async fn test_flush_session_memory_retries_invalid_persistence_decision_before_success() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = MemoryStore::new(workspace_dir.clone());
    seed_recent_user_message(&memory_store).await;

    let mut config = AgentKernelConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    flush_session_memory(
        "s1",
        "summary-model",
        Arc::new(SummaryProvider::new(vec![
            "{\"title\":\"Skill recap\",\"facts\":[\"ToolSearch activated Skill\"]}".to_string(),
            "{\"result\":\"not-a-decision\"}".to_string(),
            "{\"decision\":\"durable\",\"rationale\":\"Contains durable tool routing facts.\"}"
                .to_string(),
        ])),
        &memory_store,
        &config,
        MemoryFlushPolicy::new(false, true, true),
    )
    .await
    .expect("invalid persistence decision should retry and then succeed");

    assert_eq!(
        memory_store
            .load_summaries("s1", 10)
            .await
            .expect("summary load should succeed")
            .len(),
        1,
        "durable retry result should persist one summary"
    );
    assert!(
        workspace_dir.join("memory").exists(),
        "durable retry result should append workspace memory"
    );
}

#[tokio::test]
async fn test_flush_session_memory_errors_after_invalid_summary_retry_budget() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = MemoryStore::new(workspace_dir.clone());
    seed_recent_user_message(&memory_store).await;

    let mut config = AgentKernelConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    let error = flush_session_memory(
        "s1",
        "summary-model",
        Arc::new(SummaryProvider::new(vec![
            "still invalid".to_string(),
            "still invalid".to_string(),
            "still invalid".to_string(),
        ])),
        &memory_store,
        &config,
        MemoryFlushPolicy::new(false, true, true),
    )
    .await
    .expect_err("summary parsing should fail after retry budget");

    assert!(
        error
            .to_string()
            .contains("parse memory summary json after 3 attempts"),
        "{error}"
    );
}

#[tokio::test]
async fn test_flush_session_memory_errors_after_invalid_persistence_retry_budget() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace directory should exist");
    let memory_store = MemoryStore::new(workspace_dir.clone());
    seed_recent_user_message(&memory_store).await;

    let mut config = AgentKernelConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();

    let error = flush_session_memory(
        "s1",
        "summary-model",
        Arc::new(SummaryProvider::new(vec![
            "{\"title\":\"Skill recap\",\"facts\":[\"ToolSearch activated Skill\"]}".to_string(),
            "{\"result\":\"invalid\"}".to_string(),
            "{\"result\":\"invalid\"}".to_string(),
            "{\"result\":\"invalid\"}".to_string(),
        ])),
        &memory_store,
        &config,
        MemoryFlushPolicy::new(false, true, true),
    )
    .await
    .expect_err("persistence parsing should fail after retry budget");

    assert!(
        error
            .to_string()
            .contains("parse memory persistence decision json after 3 attempts"),
        "{error}"
    );
}
