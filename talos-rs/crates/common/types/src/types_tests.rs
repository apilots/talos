use super::*;
#[test]
fn test_agent_id() {
    let id = AgentId::new("test");
    assert_eq!(id.0, "test");

    let id2 = AgentId::new_random();
    assert!(!id2.0.is_empty());
}

#[test]
fn test_session_id() {
    let id = SessionId::new();
    assert!(!id.0.is_empty());
}

#[test]
fn test_talos_timeline_tool_progress_updates_same_item() {
    let event = talos::AgentEvent {
        event_seq: None,
        event_id: None,
        created_at_ms: None,
        event_type: talos::AgentEventType::ItemDelta,
        turn_id: "turn-1".to_string(),
        message_id: "message-1".to_string(),
        session_id: "session-1".to_string(),
        sequence: 2,
        payload: talos::AgentEventPayload::ItemDelta(talos::TalosItemDeltaEvent {
            request_id: "request-1".to_string(),
            turn_id: "turn-1".to_string(),
            item_id: "tool-1".to_string(),
            delta: talos::TalosTimelineDelta::ToolProgress {
                progress_kind: talos::TalosToolProgressKind::Stdout,
                text_delta: "running".to_string(),
            },
        }),
    };

    let encoded = serde_json::to_string(&event).expect("timeline event should encode");
    assert!(encoded.contains("\"event_type\":\"item_delta\""));
    assert!(encoded.contains("\"item_id\":\"tool-1\""));
    assert!(!encoded.contains("tool_progress_item"));

    let decoded: talos::AgentEvent =
        serde_json::from_str(&encoded).expect("timeline event should decode");
    match decoded.payload {
        talos::AgentEventPayload::ItemDelta(payload) => {
            assert_eq!(payload.item_id, "tool-1");
            assert!(matches!(
                payload.delta,
                talos::TalosTimelineDelta::ToolProgress { .. }
            ));
        }
        _ => panic!("expected item delta payload"),
    }
}

#[test]
fn test_desktop_source_type_roundtrip() {
    assert_eq!(talos::TalosSourceType::Desktop.as_str(), "desktop");
    assert!(matches!(
        talos::TalosSourceType::parse("desktop"),
        talos::TalosSourceType::Desktop
    ));

    let context = talos::TalosContext::desktop("user-1");
    assert_eq!(context.source_type, talos::TalosSourceType::Desktop);
    assert_eq!(context.provider, talos::TalosProvider::Sdk);

    let encoded = serde_json::to_string(&context).expect("context should encode");
    assert!(encoded.contains("\"source_type\":\"desktop\""));
    let decoded: talos::TalosContext =
        serde_json::from_str(&encoded).expect("context should decode");
    assert_eq!(decoded.source_type, talos::TalosSourceType::Desktop);
}

#[test]
fn test_talos_session_apply_turn_progress_updates_state_and_model() {
    let mut session = TalosSession {
        title: "Untitled Session".to_string(),
        user_id: "user-1".to_string(),
        context: SessionContext {
            session_id: "session-progress".to_string(),
            workspace_id: None,
            source_type: talos::TalosSourceType::Im,
            provider: talos::TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("channel-1".to_string()),
            thread_id: Some("thread-1".to_string()),
            project_id: Some("project-1".to_string()),
            current_directory: Some(std::path::PathBuf::from("/repo")),
            project_root: Some(std::path::PathBuf::from("/repo")),
            environment: None,
        },
        conf: SessionConf::default(),
        project_id: Some("project-1".to_string()),
        current_directory: Some(std::path::PathBuf::from("/repo")),
        config: TalosConfig::default(),
        state: SessionState {
            created_at: 100,
            last_activity: 200,
            message_count: 3,
            usage: talos::TokenUsage {
                input_tokens: 1,
                output_tokens: 2,
                total_tokens: 3,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            },
        },
    };

    session.apply_turn_progress(
        "qwen-plus",
        2,
        &talos::TokenUsage {
            input_tokens: 4,
            output_tokens: 5,
            total_tokens: 9,
            cache_creation_input_tokens: Some(7),
            cache_read_input_tokens: None,
        },
    );

    assert_eq!(session.state.message_count, 5);
    assert!(session.state.last_activity >= 200);
    assert_eq!(session.state.usage.input_tokens, 5);
    assert_eq!(session.state.usage.output_tokens, 7);
    assert_eq!(session.state.usage.total_tokens, 12);
    assert_eq!(session.state.usage.cache_creation_input_tokens, Some(7));
    assert_eq!(session.config.llm.model, "qwen-plus");
}

#[test]
fn test_session_override_policy_accepts_model_selection() {
    let conf = SessionConf {
        model_selection: Some(ModelSelection {
            model_ref: ModelRef {
                provider: "openai".to_string(),
                id: "gpt-4o".to_string(),
            },
            reasoning: ReasoningSelection::ModelDefault,
        }),
    };
    SessionConfigOverridePolicy::validate_conf(&conf)
        .expect("known strong typed conf should be accepted");
}

#[test]
fn test_session_override_policy_builds_model_selection_conf() {
    let selection = ModelSelection {
        model_ref: ModelRef {
            provider: "openai".to_string(),
            id: "gpt-4o".to_string(),
        },
        reasoning: ReasoningSelection::Effort {
            level: ReasoningEffort::High,
        },
    };
    let conf = SessionConfigOverridePolicy::single_model_selection_conf(&selection);
    let extracted = SessionConfigOverridePolicy::extract_model_selection(&conf)
        .expect("conf extraction should succeed");

    assert_eq!(extracted, Some(selection));
}

#[test]
fn test_model_selection_resolves_model_default_effort() {
    let selection = ModelSelection {
        model_ref: ModelRef {
            provider: "openai".to_string(),
            id: "gpt-5.5".to_string(),
        },
        reasoning: ReasoningSelection::ModelDefault,
    };
    let model = ModelInfo {
        id: "gpt-5.5".to_string(),
        name: "GPT-5.5".to_string(),
        provider: "openai".to_string(),
        context_window: 1_000_000,
        reasoning: true,
        reasoning_effort: ReasoningEffort::Medium,
        speed: ModelSpeed::Standard,
        input: vec![ModelInputKind::Text],
        cost: ModelCostConfig::default(),
    };

    assert_eq!(
        selection.resolve_reasoning(&model).unwrap(),
        EffectiveReasoning::Effort(ReasoningEffort::Medium)
    );
}

#[test]
fn test_model_selection_resolves_explicit_effort() {
    let selection = ModelSelection {
        model_ref: ModelRef {
            provider: "openai".to_string(),
            id: "gpt-5.5".to_string(),
        },
        reasoning: ReasoningSelection::Effort {
            level: ReasoningEffort::XHigh,
        },
    };
    let model = ModelInfo {
        id: "gpt-5.5".to_string(),
        name: "GPT-5.5".to_string(),
        provider: "openai".to_string(),
        context_window: 1_000_000,
        reasoning: true,
        reasoning_effort: ReasoningEffort::Medium,
        speed: ModelSpeed::Standard,
        input: vec![ModelInputKind::Text],
        cost: ModelCostConfig::default(),
    };

    assert_eq!(
        selection.resolve_reasoning(&model).unwrap(),
        EffectiveReasoning::Effort(ReasoningEffort::XHigh)
    );
}

#[test]
fn test_model_selection_resolves_unsupported_model_default() {
    let selection = ModelSelection {
        model_ref: ModelRef {
            provider: "ollama".to_string(),
            id: "qwen".to_string(),
        },
        reasoning: ReasoningSelection::ModelDefault,
    };
    let model = ModelInfo {
        id: "qwen".to_string(),
        name: "Qwen".to_string(),
        provider: "ollama".to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: ReasoningEffort::Medium,
        speed: ModelSpeed::Standard,
        input: vec![ModelInputKind::Text],
        cost: ModelCostConfig::default(),
    };

    assert_eq!(
        selection.resolve_reasoning(&model).unwrap(),
        EffectiveReasoning::Unsupported
    );
}

#[test]
fn test_model_selection_rejects_explicit_effort_for_unsupported_model() {
    let selection = ModelSelection {
        model_ref: ModelRef {
            provider: "ollama".to_string(),
            id: "qwen".to_string(),
        },
        reasoning: ReasoningSelection::Effort {
            level: ReasoningEffort::High,
        },
    };
    let model = ModelInfo {
        id: "qwen".to_string(),
        name: "Qwen".to_string(),
        provider: "ollama".to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: ReasoningEffort::Medium,
        speed: ModelSpeed::Standard,
        input: vec![ModelInputKind::Text],
        cost: ModelCostConfig::default(),
    };

    let error = selection
        .resolve_reasoning(&model)
        .expect_err("explicit effort should be rejected");

    assert!(matches!(error, TalosError::ValidationError(_)));
}

#[test]
fn test_model_selection_serializes_as_single_wire_object() {
    let selection = ModelSelection {
        model_ref: ModelRef {
            provider: "openai".to_string(),
            id: "gpt-5.5".to_string(),
        },
        reasoning: ReasoningSelection::Effort {
            level: ReasoningEffort::High,
        },
    };

    let encoded = serde_json::to_value(&selection).unwrap();

    assert_eq!(
        encoded,
        serde_json::json!({
            "modelRef": {"provider": "openai", "id": "gpt-5.5"},
            "reasoning": {"type": "effort", "level": "high"}
        })
    );
}

#[test]
fn test_session_title_policy_normalizes_generated_title() {
    let title =
        session_title_or_fallback("\"Plan Service Migration.\"".to_string(), "fallback text");

    assert_eq!(title, "Plan Service Migration");
}

#[test]
fn test_session_title_policy_falls_back_to_first_user_text() {
    let title = session_title_or_fallback("Untitled Session".to_string(), "  migrate phase one\n");

    assert_eq!(title, "migrate phase one");
}

#[test]
fn test_session_patch_roundtrip_with_project_binding_fields() {
    let patch = SessionPatch {
        conf: None,
        project_id: Some(OptionalProjectIdPatch::Set {
            project_id: "project-1".to_string(),
        }),
        current_directory: Some(OptionalCurrentDirectoryPatch::Set {
            path: std::path::PathBuf::from("/repo"),
        }),
    };

    let encoded = serde_json::to_string(&patch).unwrap();
    let decoded: SessionPatch = serde_json::from_str(&encoded).unwrap();

    assert_eq!(
        decoded.project_id,
        Some(OptionalProjectIdPatch::Set {
            project_id: "project-1".to_string(),
        })
    );
    assert_eq!(
        decoded.current_directory,
        Some(OptionalCurrentDirectoryPatch::Set {
            path: std::path::PathBuf::from("/repo"),
        })
    );
}

#[test]
fn test_resolve_effective_project_root_prefers_project_directory() {
    let session = TalosSession {
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: "user-1".to_string(),
        context: SessionContext {
            session_id: "session-root".to_string(),
            workspace_id: None,
            source_type: talos::TalosSourceType::Http,
            provider: talos::TalosProvider::Http,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("default".to_string()),
            thread_id: None,
            project_id: Some("project-1".to_string()),
            current_directory: Some(std::path::PathBuf::from("/current")),
            project_root: None,
            environment: None,
        },
        conf: SessionConf::default(),
        project_id: Some("project-1".to_string()),
        current_directory: Some(std::path::PathBuf::from("/current")),
        config: TalosConfig::default(),
        state: SessionState {
            created_at: 100,
            last_activity: 200,
            message_count: 0,
            usage: talos::TokenUsage::default(),
        },
    };

    let root = resolve_effective_project_root(
        &session,
        Some(std::path::PathBuf::from("/project")),
        Some(std::path::PathBuf::from("/workspace")),
    );

    assert_eq!(root.as_deref(), Some(std::path::Path::new("/project")));
}

#[test]
fn test_resolve_effective_project_root_uses_embedded_current_directory() {
    let session = TalosSession {
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: "user-1".to_string(),
        context: SessionContext {
            session_id: "session-root".to_string(),
            workspace_id: None,
            source_type: talos::TalosSourceType::Http,
            provider: talos::TalosProvider::Http,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("default".to_string()),
            thread_id: None,
            project_id: None,
            current_directory: Some(std::path::PathBuf::from("/current")),
            project_root: None,
            environment: Some(talos::TalosSessionEnvironment {
                directory: std::path::PathBuf::from("/current"),
                connection_mode: talos::TalosConnectionMode::Embedded,
            }),
        },
        conf: SessionConf::default(),
        project_id: None,
        current_directory: Some(std::path::PathBuf::from("/current")),
        config: TalosConfig::default(),
        state: SessionState {
            created_at: 100,
            last_activity: 200,
            message_count: 0,
            usage: talos::TokenUsage::default(),
        },
    };

    let root = resolve_effective_project_root(
        &session,
        None,
        Some(std::path::PathBuf::from("/workspace")),
    );

    assert_eq!(root.as_deref(), Some(std::path::Path::new("/current")));
}

#[test]
fn test_resolve_effective_project_root_uses_workspace_root() {
    let session = TalosSession {
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: "user-1".to_string(),
        context: SessionContext {
            session_id: "session-root".to_string(),
            workspace_id: Some("workspace-alpha".to_string()),
            source_type: talos::TalosSourceType::Http,
            provider: talos::TalosProvider::Http,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("default".to_string()),
            thread_id: None,
            project_id: None,
            current_directory: None,
            project_root: None,
            environment: None,
        },
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
        config: TalosConfig::default(),
        state: SessionState {
            created_at: 100,
            last_activity: 200,
            message_count: 0,
            usage: talos::TokenUsage::default(),
        },
    };

    let root = resolve_effective_project_root(
        &session,
        None,
        Some(std::path::PathBuf::from("/workspace")),
    );

    assert_eq!(root.as_deref(), Some(std::path::Path::new("/workspace")));
}

#[test]
fn test_doc_update_request_roundtrip() {
    let request = DocUpdateRequest {
        content: "hello".to_string(),
        base_version: ExpectedFileVersion::Match {
            version_token: "token-1".to_string(),
        },
    };

    let encoded = serde_json::to_string(&request).unwrap();
    let decoded: DocUpdateRequest = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, request);
}

#[test]
fn test_memory_file_summary_roundtrip() {
    let summary = MemoryFileSummary {
        date: "2026-04-17".to_string(),
        section_count: 3,
        updated_at: Some(123),
        version_token: "token-1".to_string(),
    };

    let encoded = serde_json::to_string(&summary).unwrap();
    let decoded: MemoryFileSummary = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, summary);
}

#[test]
fn test_workspace_doc_kind_file_name() {
    assert_eq!(WorkspaceDocKind::Soul.file_name(), "SOUL.md");
    assert_eq!(WorkspaceDocKind::Identity.file_name(), "IDENTITY.md");
    assert_eq!(WorkspaceDocKind::User.file_name(), "USER.md");
    assert_eq!(WorkspaceDocKind::Heartbeat.file_name(), "HEARTBEAT.md");
    assert_eq!(WorkspaceDocKind::Memory.file_name(), "MEMORY.md");
}

#[test]
fn test_workspace_doc_detail_roundtrip() {
    let detail = WorkspaceDocInfo {
        kind: WorkspaceDocKind::Memory,
        path: "MEMORY.md".to_string(),
        exists: true,
        content: "content".to_string(),
        updated_at: Some(123),
        version_token: Some("token-1".to_string()),
    };

    let encoded = serde_json::to_string(&detail).unwrap();
    let decoded: WorkspaceDocInfo = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, detail);
}

#[test]
fn test_memory_file_detail_roundtrip() {
    let detail = MemoryFileDetail {
        date: "2026-04-17".to_string(),
        section_count: 4,
        content: "content".to_string(),
        updated_at: Some(123),
        version_token: "token-1".to_string(),
    };

    let encoded = serde_json::to_string(&detail).unwrap();
    let decoded: MemoryFileDetail = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, detail);
}
