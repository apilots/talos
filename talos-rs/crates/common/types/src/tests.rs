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
fn test_talos_roundtrip() {
    let req = talos::TalosMessageRequest {
        input: vec![talos::TalosInputItem::Text {
            text: "hello".to_string(),
        }],
        stream: Some(false),
        workspace_id: Some("workspace-1".to_string()),
    };

    let encoded = serde_json::to_string(&req).unwrap();
    let decoded: talos::TalosMessageRequest = serde_json::from_str(&encoded).unwrap();
    assert!(decoded.stream.is_some());
    assert_eq!(decoded.workspace_id.as_deref(), Some("workspace-1"));
    assert!(matches!(
        decoded.input.first(),
        Some(talos::TalosInputItem::Text { text }) if text == "hello"
    ));
}

#[test]
fn test_session_context_and_state_from_record() {
    let record = SessionRecord {
        id: "session-1".to_string(),
        title: "Untitled Session".to_string(),
        context: talos::TalosContext {
            source_type: talos::TalosSourceType::Im,
            provider: talos::TalosProvider::Telegram,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("channel-1".to_string()),
            thread_id: Some("thread-1".to_string()),
            project_root: Some(std::path::PathBuf::from("/repo")),
            environment: None,
        },
        model: "qwen-max".to_string(),
        created_at: 100,
        last_activity: 200,
        message_count: 3,
        usage: talos::TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: Some("project-1".to_string()),
        current_directory: Some(std::path::PathBuf::from("/repo")),
    };

    let context = SessionContext::from_record(&record, Some("workspace-1".to_string()));
    let state = SessionState::from_record(&record);

    assert_eq!(context.session_id, "session-1");
    assert_eq!(context.workspace_id.as_deref(), Some("workspace-1"));
    assert_eq!(
        context.current_directory.as_deref(),
        Some(std::path::Path::new("/repo"))
    );
    assert_eq!(context.project_id.as_deref(), Some("project-1"));
    assert_eq!(context.provider, talos::TalosProvider::Telegram);
    assert_eq!(state.created_at, 100);
    assert_eq!(state.last_activity, 200);
    assert_eq!(state.message_count, 3);
    assert_eq!(state.usage.total_tokens, 0);
}

#[test]
fn test_talos_session_from_record_applies_conf_override() {
    let mut record = SessionRecord {
        id: "session-1".to_string(),
        title: "Untitled Session".to_string(),
        context: talos::TalosContext {
            source_type: talos::TalosSourceType::Http,
            provider: talos::TalosProvider::Http,
            user_id: "user-1".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: Some("default".to_string()),
            thread_id: None,
            project_root: None,
            environment: None,
        },
        model: "qwen-max".to_string(),
        created_at: 100,
        last_activity: 200,
        message_count: 3,
        usage: talos::TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
    };
    record.conf = SessionConfigOverridePolicy::single_model_conf("qwen-plus");

    let session = TalosSession::from_record(&record, &TalosConfig::default(), None)
        .expect("talos session restore should succeed");
    assert_eq!(session.title, "Untitled Session");
    assert_eq!(session.config.agent.model, "qwen-plus");
    assert_eq!(session.config.llm.model, "qwen-plus");
}

#[test]
fn test_session_override_policy_rejects_unknown_keys() {
    let conf = SessionConf {
        agent: Some(SessionAgentConf {
            model: Some("gpt-4o".to_string()),
        }),
    };
    SessionConfigOverridePolicy::validate_conf(&conf)
        .expect("known strong typed conf should be accepted");
}

#[test]
fn test_session_override_policy_builds_model_conf() {
    let conf = SessionConfigOverridePolicy::single_model_conf("gpt-4o");
    let model = SessionConfigOverridePolicy::extract_agent_model(&conf)
        .expect("conf extraction should succeed");

    assert_eq!(model.as_deref(), Some("gpt-4o"));
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
        path: "memory/2026-04-17.md".to_string(),
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
    let detail = WorkspaceDocDetail {
        kind: WorkspaceDocKind::Memory,
        path: "MEMORY.md".to_string(),
        exists: true,
        content: "content".to_string(),
        updated_at: Some(123),
        version_token: Some("token-1".to_string()),
    };

    let encoded = serde_json::to_string(&detail).unwrap();
    let decoded: WorkspaceDocDetail = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, detail);
}

#[test]
fn test_memory_file_detail_roundtrip() {
    let detail = MemoryFileDetail {
        date: "2026-04-17".to_string(),
        path: "memory/2026-04-17.md".to_string(),
        section_count: 4,
        content: "content".to_string(),
        updated_at: Some(123),
        version_token: "token-1".to_string(),
    };

    let encoded = serde_json::to_string(&detail).unwrap();
    let decoded: MemoryFileDetail = serde_json::from_str(&encoded).unwrap();

    assert_eq!(decoded, detail);
}
