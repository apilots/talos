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

struct SameTurnMessageWindows {
    messages: Vec<crate::SessionPromptMessage>,
}

impl SessionPromptContextSource for SameTurnMessageWindows {
    fn load(&self, config: SessionPromptContextConfig) -> SessionPromptContextFuture<'_> {
        Box::pin(async move {
            let after = config
                .compact_cursor
                .map_or(0, |cursor| cursor.covers_until_seq);
            let messages = self
                .messages
                .iter()
                .enumerate()
                .filter(|(index, _)| *index as u64 + 1 > after)
                .take(
                    config
                        .source_message_limit
                        .unwrap_or(config.recent_messages_limit)
                        .min(config.recent_messages_limit),
                )
                .collect::<Vec<_>>();
            let Some((last, _)) = messages.last() else {
                return Ok(Default::default());
            };
            let position = *last + 1;
            Ok(crate::SessionPromptContext {
                source_turns: vec![crate::SessionPromptSourceTurn {
                    turn_id: "same-turn".into(),
                    completed_event_seq: if position == self.messages.len() {
                        position as i64 + 1
                    } else {
                        position as i64
                    },
                    completed_at: 1,
                }],
                recent_messages: messages
                    .into_iter()
                    .map(|(_, message)| message.clone())
                    .collect(),
                ..Default::default()
            })
        })
    }
}

#[tokio::test]
async fn dream_memory_pages_one_turn_when_its_message_group_exceeds_the_transcript_budget() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).unwrap();
    let memory = TestMemoryStore::new();
    let source = SameTurnMessageWindows {
        messages: ["FIRST", "SECOND", "THIRD"]
            .into_iter()
            .enumerate()
            .map(|(index, marker)| crate::SessionPromptMessage {
                header: crate::SessionPromptEntryHeader {
                    id: format!("message-{index}"),
                    turn_id: "same-turn".into(),
                    completed_at: 1,
                },
                role: SessionPromptMessageRole::User,
                text: format!("{marker} {}", "x".repeat(3_000)),
            })
            .collect(),
    };
    let provider = Arc::new(SummaryProvider::new(
        vec![r#"{"mode":"maintenance","memory_candidates":[]}"#.into(); 2],
    ));
    let config = AgentKernelConfig {
        workspace_dir: workspace.to_string_lossy().into(),
        ..Default::default()
    };
    let scope = MemoryScope {
        workspace_id: "default".into(),
        user_id: "user-1".into(),
        workspace_root: workspace.clone(),
    };
    let mut coverage = Vec::new();
    for _ in 0..2 {
        let report = crate::form_dream_session_memory(
            MemoryFormationParams {
                session_id: "s1",
                producer_agent_id: "agent-1",
                dream_memory_store: &memory,
                memory_scope: &scope,
                session_artifact_store: &memory,
                session_prompt_context_source: &source,
                memory_progress_store: &memory,
                kernel_config: &config,
            },
            model_call(provider.clone()),
        )
        .await
        .unwrap();
        coverage.push(report.covers_until_seq);
    }
    assert_eq!(coverage, vec![Some(2), Some(4)]);
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0].messages[1].content.contains("FIRST")
            && requests[0].messages[1].content.contains("SECOND")
            && !requests[0].messages[1].content.contains("THIRD")
    );
    assert!(requests[1].messages[1].content.contains("THIRD"));
    std::fs::remove_dir_all(workspace).unwrap();
}

#[tokio::test]
async fn compaction_reloads_a_smaller_same_turn_prefix_with_its_model_budget() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).unwrap();
    let memory = TestMemoryStore::new();
    let source = SameTurnMessageWindows {
        messages: ["FIRST", "SECOND", "THIRD"]
            .into_iter()
            .enumerate()
            .map(|(index, marker)| SessionPromptMessage {
                header: SessionPromptEntryHeader {
                    id: format!("message-{index}"),
                    turn_id: "same-turn".into(),
                    completed_at: 1,
                },
                role: SessionPromptMessageRole::User,
                text: format!("{marker} {}", "x".repeat(1_500)),
            })
            .collect(),
    };
    let mut model = build_ctx_with_stream_for_session("s1", None).selected_model_info;
    model.model.context_window = 2_048;
    let provider = Arc::new(SummaryProvider::new(
        vec![r#"{"mode":"compaction","title":"Bounded prefix","memory_candidates":[]}"#.into(); 2],
    ));
    let config = AgentKernelConfig {
        workspace_dir: workspace.to_string_lossy().into(),
        ..Default::default()
    };
    let scope = MemoryScope {
        workspace_id: "default".into(),
        user_id: "user-1".into(),
        workspace_root: workspace.clone(),
    };
    let mut coverage = Vec::new();
    for _ in 0..2 {
        let report = crate::compact_session_memory(
            MemoryFormationParams {
                session_id: "s1",
                producer_agent_id: "agent-1",
                dream_memory_store: &memory,
                memory_scope: &scope,
                session_artifact_store: &memory,
                session_prompt_context_source: &source,
                memory_progress_store: &memory,
                kernel_config: &config,
            },
            "summary-model",
            provider.clone(),
            MemoryCompactionPolicy::for_model_compaction(&model),
        )
        .await
        .unwrap();
        coverage.push(report.covers_until_seq);
    }
    assert_eq!(coverage, vec![Some(2), Some(4)]);
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0].messages[1].content.contains("SECOND")
            && !requests[0].messages[1].content.contains("THIRD")
    );
    assert!(requests[1].messages[1].content.contains("THIRD"));
    std::fs::remove_dir_all(workspace).unwrap();
}

#[tokio::test]
async fn compaction_rejects_one_message_beyond_the_model_budget_without_advancing() {
    let workspace = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace).unwrap();
    let memory = TestMemoryStore::new();
    let source = SameTurnMessageWindows {
        messages: vec![SessionPromptMessage {
            header: SessionPromptEntryHeader {
                id: "large".into(),
                turn_id: "same-turn".into(),
                completed_at: 1,
            },
            role: SessionPromptMessageRole::User,
            text: "x".repeat(2_000),
        }],
    };
    let mut model = build_ctx_with_stream_for_session("s1", None).selected_model_info;
    model.model.context_window = 256;
    let provider = Arc::new(SummaryProvider::new(vec![]));
    let config = AgentKernelConfig {
        workspace_dir: workspace.to_string_lossy().into(),
        ..Default::default()
    };
    let scope = MemoryScope {
        workspace_id: "default".into(),
        user_id: "user-1".into(),
        workspace_root: workspace.clone(),
    };
    let error = crate::compact_session_memory(
        MemoryFormationParams {
            session_id: "s1",
            producer_agent_id: "agent-1",
            dream_memory_store: &memory,
            memory_scope: &scope,
            session_artifact_store: &memory,
            session_prompt_context_source: &source,
            memory_progress_store: &memory,
            kernel_config: &config,
        },
        "summary-model",
        provider.clone(),
        MemoryCompactionPolicy::for_model_compaction(&model),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.error_code(),
        talos_error::ErrorCode::ContextWindowExceeded
    );
    assert_eq!(memory.processed_until_event_seq("s1").await.unwrap(), 0);
    assert!(provider.requests.lock().unwrap().is_empty());
    assert!(memory.writes().is_empty());
    std::fs::remove_dir_all(workspace).unwrap();
}
