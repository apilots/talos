use super::*;

#[tokio::test]

async fn test_prompt_builder_creates_and_injects_bootstrap_static_sections() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let project_root = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&project_root).expect("project root should exist");
    std::fs::write(
        project_root.join("AGENTS.md"),
        "# AGENTS\n\n- Prefer crate roots before docs.\n",
    )
    .expect("agents file should write");

    let mut config = PromptBuildConfig {
        workspace_dir: workspace_dir.to_string_lossy().to_string(),
        ..PromptBuildConfig::default()
    };
    config.load_policy.include_session_summary = false;
    config.load_policy.include_recent_messages = false;

    let assembly = PromptBuilder::new()
        .project_root(Some(project_root))
        .config(config)
        .user_input("bootstrap")
        .assemble()
        .await
        .expect("prompt build should succeed");

    for name in ["SOUL.md", "USER.md"] {
        assert!(
            workspace_dir.join(name).exists(),
            "expected {} to exist",
            name
        );
    }

    let static_text = render_prompt_blocks(&assembly)
        .into_iter()
        .filter(|block| block.kind == PromptBlockKind::StaticPrefix)
        .map(|block| block.text)
        .collect::<Vec<_>>()
        .join("\n\n");
    assert!(static_text.contains("# Bootstrap: AGENTS.md"));
    assert!(static_text.contains("# Bootstrap: SOUL.md"));
    assert!(!workspace_dir.join("MEMORY.md").exists());
    assert!(!static_text.contains("# Bootstrap: MEMORY.md"));
    assert!(static_text.contains("# User Workspace Memory Policy"));
}

#[tokio::test]
async fn test_prompt_builder_minimal_mode_filters_bootstrap_files() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let project_root = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should exist");
    std::fs::create_dir_all(&project_root).expect("project root should exist");
    std::fs::write(
        project_root.join("AGENTS.md"),
        "# AGENTS\n\n- Project rule.\n",
    )
    .expect("agents file should write");

    let mut config = PromptBuildConfig {
        workspace_dir: workspace_dir.to_string_lossy().to_string(),
        ..PromptBuildConfig::default()
    };
    config.load_policy.include_session_summary = false;
    config.load_policy.include_recent_messages = false;

    let assembly = PromptBuilder::new()
        .mode(PromptMode::Minimal)
        .project_root(Some(project_root))
        .config(config)
        .user_input("bootstrap")
        .assemble()
        .await
        .expect("prompt build should succeed");

    let static_text = render_prompt_blocks(&assembly)
        .into_iter()
        .filter(|block| block.kind == PromptBlockKind::StaticPrefix)
        .map(|block| block.text)
        .collect::<Vec<_>>()
        .join("\n\n");
    assert!(static_text.contains("# Bootstrap: AGENTS.md"));
    assert!(static_text.contains("# Bootstrap: SOUL.md"));
    assert!(static_text.contains("# Bootstrap: USER.md"));
    assert!(!static_text.contains("# Bootstrap: MEMORY.md"));
    assert!(!static_text.contains("# Bootstrap: HEARTBEAT.md"));
    assert!(!static_text.contains("# Bootstrap: IDENTITY.md"));
}

#[tokio::test]
async fn test_prompt_builder_falls_back_to_workspace_agents_when_project_agents_missing() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let project_root = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should exist");
    std::fs::create_dir_all(&project_root).expect("project root should exist");
    std::fs::write(
        workspace_dir.join("AGENTS.md"),
        "# AGENTS\n\n- Workspace fallback rule.\n",
    )
    .expect("workspace agents file should write");

    let mut config = PromptBuildConfig {
        workspace_dir: workspace_dir.to_string_lossy().to_string(),
        ..PromptBuildConfig::default()
    };
    config.load_policy.include_session_summary = false;
    config.load_policy.include_recent_messages = false;

    let assembly = PromptBuilder::new()
        .project_root(Some(project_root))
        .config(config)
        .user_input("bootstrap")
        .assemble()
        .await
        .expect("prompt assembly should succeed");

    let bootstrap_agents = assembly
        .static_prefix_sections
        .iter()
        .find(|section| section.kind == PromptSectionKind::BootstrapAgents)
        .expect("bootstrap agents should exist");
    assert!(bootstrap_agents.payload.contains("Workspace fallback rule"));
}

#[tokio::test]
async fn test_prompt_builder_uses_absent_placeholder_for_missing_bootstrap_slot() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());

    let config = PromptBuildConfig {
        workspace_dir: workspace_dir.to_string_lossy().to_string(),
        ..PromptBuildConfig::default()
    };

    let assembly = PromptBuilder::new()
        .config(config)
        .user_input("bootstrap")
        .assemble()
        .await
        .expect("prompt assembly should succeed");

    let heartbeat = assembly
        .static_prefix_sections
        .iter()
        .find(|section| section.kind == PromptSectionKind::BootstrapHeartbeat)
        .expect("heartbeat bootstrap slot should exist");
    assert!(heartbeat.payload.contains("file_state: absent"));
}

#[tokio::test]
async fn test_dynamic_user_workspace_memory_is_required_bounded_and_does_not_change_static_cache() {
    let first = PromptBuilder::new()
        .user_workspace_memory(UserWorkspaceMemoryPromptContext {
            file_state: UserWorkspaceMemoryPromptFileState::Valid,
            version_token: Some("revision-1".to_string()),
            content: "first memory".to_string(),
        })
        .user_input("bootstrap")
        .assemble()
        .await
        .expect("first prompt should build");
    let second = PromptBuilder::new()
        .user_workspace_memory(UserWorkspaceMemoryPromptContext {
            file_state: UserWorkspaceMemoryPromptFileState::Invalid,
            version_token: Some("revision-2".to_string()),
            content: "x".repeat(talos_types::MEMORY_FILE_MAX_CHARS + 100),
        })
        .user_input("bootstrap")
        .assemble()
        .await
        .expect("second prompt should build");

    let memory = second
        .dynamic_suffix_sections
        .iter()
        .find(|section| section.kind == PromptSectionKind::UserWorkspaceMemory)
        .expect("dynamic memory should be required");
    assert!(memory.payload.contains("file_state: invalid"));
    assert!(memory.payload.contains("revision-2"));
    assert!(memory.payload.chars().count() <= talos_types::MEMORY_FILE_MAX_CHARS + 100);
    assert_eq!(
        first.cache_state.static_prefix_fingerprint,
        second.cache_state.static_prefix_fingerprint
    );
}
