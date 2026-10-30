use super::*;
use talos_llm_provider::{MessageInputKind, MessageInputOrigin, MessageInputSource};

#[tokio::test]

async fn test_prepended_user_context_message_renders_as_user_message() {
    let assembly = PromptBuilder::new()
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt build should succeed");

    let messages = crate::renderer::render_chat_messages(&render_prompt_blocks(&assembly));
    let prepended = messages
        .iter()
        .find(|message| {
            message.role == Role::User && message.content.contains("[PrependedUserContextMessage]")
        })
        .expect("prepended user context should render as user message");

    assert!(prepended.content.contains("<system-reminder>"));
    assert_eq!(
        prepended.input_origin,
        Some(MessageInputOrigin {
            kind: MessageInputKind::Context,
            source: MessageInputSource::Runtime,
        })
    );
}

#[tokio::test]
async fn test_dynamic_suffix_renders_as_system_message() {
    let assembly = PromptBuilder::new()
        .planning_instruction(Some("inspect the repository".to_string()))
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt build should succeed");

    let messages = crate::renderer::render_chat_messages(&render_prompt_blocks(&assembly));
    let dynamic = messages
        .iter()
        .find(|message| message.content.contains("# Planning Instruction"))
        .expect("dynamic suffix message should exist");

    assert_eq!(dynamic.role, Role::System);
    assert_eq!(
        dynamic.input_origin,
        Some(MessageInputOrigin {
            kind: MessageInputKind::Context,
            source: MessageInputSource::Mixed,
        })
    );
}

#[tokio::test]
async fn test_appended_system_context_renders_as_system_message() {
    let assembly = PromptBuilder::new()
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt build should succeed");

    let messages = crate::renderer::render_chat_messages(&render_prompt_blocks(&assembly));
    let appended = messages
        .iter()
        .find(|message| message.content.contains("[AppendedSystemContext]"))
        .expect("appended system context should exist");

    assert_eq!(appended.role, Role::System);
    assert_eq!(
        appended.input_origin,
        Some(MessageInputOrigin {
            kind: MessageInputKind::Context,
            source: MessageInputSource::Runtime,
        })
    );
}

#[tokio::test]
async fn test_rendered_cache_breakpoints_stay_before_user_input() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));

    let assembly = PromptBuilder::new()
        .phase(PromptPhase::Planning)
        .planner_mode("built_in")
        .expect("planner mode should parse")
        .planning_instruction(Some("build a short plan first".to_string()))
        .tools(&registry)
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt assembly should succeed");

    let rendered = render_prompt_assembly(&assembly);
    let last_cached_index = rendered
        .iter()
        .enumerate()
        .filter_map(|(index, message)| message.cache_control.as_ref().map(|_| index))
        .max()
        .expect("at least one cached message should exist");

    assert!(last_cached_index < rendered.len() - 1);
    assert_eq!(
        rendered.last().expect("user message should exist").role,
        Role::User
    );
    assert_eq!(
        rendered.last().unwrap().input_origin,
        Some(MessageInputOrigin {
            kind: MessageInputKind::User,
            source: MessageInputSource::User,
        })
    );
}

#[tokio::test]
async fn test_static_prefix_and_bootstrap_blocks_are_cacheable() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());

    let config = PromptBuildConfig {
        workspace_dir: workspace_dir.to_string_lossy().to_string(),
        ..PromptBuildConfig::default()
    };

    let assembly = PromptBuilder::new()
        .config(config)
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt assembly should succeed");

    let blocks = render_prompt_blocks(&assembly);
    let static_block = blocks
        .iter()
        .find(|block| block.kind == PromptBlockKind::StaticPrefix)
        .expect("static prefix should exist");
    assert!(static_block.cache_scope.is_cacheable());
    assert_eq!(
        static_block.input_origin,
        Some(MessageInputOrigin {
            kind: MessageInputKind::System,
            source: MessageInputSource::Prompt,
        })
    );
    let bootstrap = blocks
        .iter()
        .find(|block| block.cache_scope == PromptBlockCacheScope::ScopedReusable)
        .expect("workspace bootstrap should keep its own cache group");
    assert_eq!(
        bootstrap.input_origin,
        Some(MessageInputOrigin {
            kind: MessageInputKind::System,
            source: MessageInputSource::Workspace,
        })
    );
}

#[tokio::test]
async fn test_prompt_cache_break_reason_detects_tool_catalog_changes() {
    let baseline = PromptBuilder::new()
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt build should succeed");

    let registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));
    let changed = PromptBuilder::new()
        .tools(&registry)
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt build should succeed");

    assert_eq!(
        determine_prompt_cache_break_reason(&baseline.cache_state, &changed.cache_state),
        Some(PromptCacheBreakReason::ToolCatalogChanged)
    );
}

#[tokio::test]
async fn test_prompt_cache_break_reason_detects_bootstrap_changes() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let config = PromptBuildConfig {
        workspace_dir: workspace_dir.to_string_lossy().to_string(),
        ..PromptBuildConfig::default()
    };

    let baseline = PromptBuilder::new()
        .config(config.clone())
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt build should succeed");

    std::fs::write(
        workspace_dir.join("SOUL.md"),
        "# SOUL\n\n- Updated bootstrap guidance.\n",
    )
    .expect("bootstrap file should write");
    let changed = PromptBuilder::new()
        .config(config)
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt build should succeed");

    assert_eq!(
        determine_prompt_cache_break_reason(&baseline.cache_state, &changed.cache_state),
        Some(PromptCacheBreakReason::BootstrapChanged)
    );
}

#[tokio::test]
async fn test_prompt_builder_splits_requested_tool_preference_template_and_hint() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(GrepTool));

    let assembly = PromptBuilder::new()
        .tools(&registry)
        .user_input("Analyze the repository architecture and use Grep first.")
        .assemble()
        .await
        .expect("prompt assembly should succeed");

    let static_block = render_prompt_blocks(&assembly)
        .into_iter()
        .find(|block| block.kind == PromptBlockKind::StaticPrefix)
        .expect("static block should exist");
    assert!(static_block
        .text
        .contains("# Requested Tool Preference Policy"));

    let dynamic_block = render_prompt_blocks(&assembly)
        .into_iter()
        .find(|block| block.kind == PromptBlockKind::DynamicSuffix)
        .expect("dynamic block should exist");
    assert!(dynamic_block.text.contains("requested_tools: Grep"));
}

#[tokio::test]
async fn test_appended_system_context_includes_runtime_policy_and_git_state() {
    let project_root = std::env::temp_dir().join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&project_root).expect("project root should exist");
    Command::new("git")
        .arg("init")
        .arg(&project_root)
        .output()
        .expect("git init should run");
    std::fs::write(project_root.join("tracked.txt"), "pending\n")
        .expect("workspace file should write");

    let assembly = PromptBuilder::new()
        .project_root(Some(project_root))
        .user_input("context")
        .assemble()
        .await
        .expect("prompt assembly should succeed");

    let appended_context = render_prompt_blocks(&assembly)
        .into_iter()
        .find(|block| block.kind == PromptBlockKind::AppendedSystemContext)
        .expect("appended system context should exist")
        .text;
    assert!(appended_context.contains("- git_branch: "));
    assert!(appended_context.contains("- git_status_summary: modified 1 file(s)"));
    assert!(appended_context.contains("- has_uncommitted_changes: true"));
    assert!(appended_context.contains("- configured_sandbox_request: use_default"));
    assert!(appended_context.contains("- configured_user_permission_mode: full_access"));
    assert!(!appended_context.contains("- session_mode:"));
    assert!(!appended_context.contains("- network_access:"));
}
