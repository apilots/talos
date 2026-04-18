use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use talos_llm_provider::Role;
use talos_memory::{Memory, MemoryStore, TalosTurn};
use talos_tools::{GrepTool, Tool, ToolContext, ToolOutputPayload, ToolRegistry};
use talos_types::talos::{StopReason, TalosOutputItem, TalosUsage};
use talos_types::ModelInfo;
use uuid::Uuid;

use super::*;

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Echo input for testing"
    }

    async fn execute_payload(
        &self,
        _input: Value,
        _context: ToolContext,
    ) -> talos_error::TalosResult<ToolOutputPayload> {
        Ok(ToolOutputPayload::success_text("ok"))
    }
}

struct VerboseTool {
    name: &'static str,
    description: &'static str,
}

#[async_trait]
impl Tool for VerboseTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        self.description
    }

    async fn execute_payload(
        &self,
        _input: Value,
        _context: ToolContext,
    ) -> talos_error::TalosResult<ToolOutputPayload> {
        Ok(ToolOutputPayload::success_text("ok"))
    }
}

#[test]
fn test_prompt_template_creation() {
    let template = PromptTemplate::new(
        "Hello, {{name}}! You are a {{role}}.".to_string(),
        "Simple greeting template".to_string(),
    );

    assert_eq!(
        template.required_vars,
        vec!["name".to_string(), "role".to_string()]
    );
    assert_eq!(template.description, "Simple greeting template");
}

#[test]
fn test_prompt_template_render() {
    let template = PromptTemplate::new(
        "Hello, {{name}}! You are a {{role}}.".to_string(),
        "Simple greeting template".to_string(),
    );

    let mut context = std::collections::HashMap::new();
    context.insert("name".to_string(), "Alice".to_string());
    context.insert("role".to_string(), "developer".to_string());

    let result = template.render(&context).unwrap();
    assert_eq!(result, "Hello, Alice! You are a developer.");
}

#[test]
fn test_prompt_template_missing_variable() {
    let template = PromptTemplate::new(
        "Hello, {{name}}! You are a {{role}}.".to_string(),
        "Simple greeting template".to_string(),
    );

    let mut context = std::collections::HashMap::new();
    context.insert("name".to_string(), "Alice".to_string());

    let result = template.render(&context);
    assert!(result.is_err());
}

#[test]
fn test_prompt_manager() {
    let mut manager = PromptManager::new();

    let template = PromptTemplate::new(
        "Hello, {{name}}!".to_string(),
        "Greeting template".to_string(),
    );

    manager.register_template("greeting", template);

    let mut context = std::collections::HashMap::new();
    context.insert("name".to_string(), "World".to_string());

    let result = manager.render_template("greeting", &context).unwrap();
    assert_eq!(result, "Hello, World!");
}

#[test]
fn test_extract_variables() {
    let template = "{{greeting}}, {{name}}! You have {{count}} messages.".to_string();
    let vars = PromptTemplate::extract_variables(&template);

    let expected = vec!["greeting", "name", "count"];
    assert_eq!(vars.len(), 3);
    for var in &expected {
        assert!(vars.contains(&var.to_string()));
    }
}

#[test]
fn test_presets() {
    let system_prompt = presets::default_system_prompt();
    assert!(system_prompt
        .template
        .contains("You are a helpful AI assistant"));

    let tool_prompt = presets::tool_usage_prompt();
    assert!(tool_prompt.template.contains("{{available_tools}}"));
}

#[test]
fn test_prompt_context() {
    let context = PromptContext::new()
        .add("user", "Alice")
        .add("task", "coding");

    assert_eq!(context.get("user"), Some(&"Alice".to_string()));
    assert_eq!(context.get("task"), Some(&"coding".to_string()));
}

#[tokio::test]
async fn test_prompt_builder_basic() {
    let assembly = PromptBuilder::new()
        .agent("talos", "assistant")
        .phase(PromptPhase::Execution)
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt build should succeed");

    assert!(assembly.static_prefix_sections.len() >= 4);
    assert_eq!(assembly.user_input, "hello");
    let messages = render_prompt_assembly(&assembly);
    assert_eq!(
        messages.last().expect("user message should exist").role,
        Role::User
    );
    assert_eq!(
        messages.last().expect("user message should exist").content,
        "hello"
    );
}

#[tokio::test]
async fn test_prompt_builder_includes_tool_summary_and_planning_instruction() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));

    let messages = PromptBuilder::new()
        .agent("talos", "assistant")
        .phase(PromptPhase::Planning)
        .planner_mode("plan_react")
        .expect("planner mode should parse")
        .planning_instruction(Some("build a short plan first".to_string()))
        .tools(&registry)
        .user_input("help me")
        .build()
        .await
        .expect("prompt build should succeed");

    assert!(messages
        .iter()
        .any(|message| message.content.contains("echo")));
    assert!(messages
        .iter()
        .any(|message| message.content.contains("Planning instruction")));
}

#[tokio::test]
async fn test_prompt_builder_keeps_history_without_legacy_budget_caps() {
    let memory = MemoryStore::new(std::env::temp_dir().join(Uuid::new_v4().to_string()));
    for idx in 0..6 {
        let turn_id = format!("turn-{idx}");
        let timestamp = Utc::now();
        memory
            .append_turn(&TalosTurn {
                turn_id: turn_id.clone(),
                session_id: "s1".to_string(),
                seq: memory.next_turn_seq("s1").await.expect("seq should load"),
                timestamp,
                items: vec![
                    TalosOutputItem::UserInput {
                        text: format!("historical user {}", idx),
                    },
                    TalosOutputItem::OutputText {
                        text: format!("historical message {}", idx),
                    },
                ],
                usage: TalosUsage::default(),
                stop_reason: StopReason::EndTurn,
            })
            .await
            .expect("append should succeed");
    }

    let assembly = PromptBuilder::new()
        .agent("talos", "assistant")
        .memory_store("s1", &memory)
        .user_input("latest")
        .assemble()
        .await
        .expect("prompt build should succeed");

    assert!(assembly
        .dynamic_suffix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::RecentMessages));
    assert!(assembly.diagnostics.dropped_sections.is_empty());
    assert!(assembly.diagnostics.truncated_sections.is_empty());
}

#[tokio::test]
async fn test_prompt_builder_keeps_static_sections_without_legacy_budget_caps() {
    let assembly = PromptBuilder::new()
        .agent("talos", "assistant")
        .planning_instruction(Some("keep it short".to_string()))
        .user_input("latest")
        .assemble()
        .await
        .expect("prompt build should succeed");

    assert!(assembly
        .dynamic_suffix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::PlanningInstruction));
    assert!(assembly.diagnostics.dropped_sections.is_empty());
    assert!(assembly.diagnostics.truncated_sections.is_empty());
}

#[tokio::test]
async fn test_prompt_builder_resolves_effective_prompt_budget_from_model_info() {
    let assembly = PromptBuilder::new()
        .agent("talos", "assistant")
        .model_info(ModelInfo {
            id: "qwen-max".to_string(),
            name: "qwen-max".to_string(),
            provider: "bailian".to_string(),
            context_window: 32_768,
            max_tokens: 8_192,
            reasoning: false,
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        })
        .user_input("latest")
        .assemble()
        .await
        .expect("prompt build should succeed");

    let window_usage = &assembly.diagnostics.window_usage;
    assert!(window_usage.window_resolved);
    assert_eq!(window_usage.context_window, 32_768);
    assert_eq!(window_usage.reserved_output_tokens, 8_192);
    assert_eq!(window_usage.compaction_buffer_tokens, 4_096);
    assert_eq!(window_usage.effective_prompt_budget, 20_480);
    assert_eq!(
        window_usage.total_tokens,
        assembly.diagnostics.total_estimated_tokens
    );
}

#[tokio::test]
async fn test_prompt_builder_caps_compaction_buffer_for_small_context_windows() {
    let assembly = PromptBuilder::new()
        .agent("talos", "assistant")
        .model_info(ModelInfo {
            id: "minimax-m2.5".to_string(),
            name: "minimax-m2.5".to_string(),
            provider: "minimax".to_string(),
            context_window: 4_096,
            max_tokens: 2_048,
            reasoning: false,
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        })
        .user_input("latest")
        .assemble()
        .await
        .expect("prompt build should succeed");

    let window_usage = &assembly.diagnostics.window_usage;
    assert!(window_usage.window_resolved);
    assert_eq!(window_usage.context_window, 4_096);
    assert_eq!(window_usage.reserved_output_tokens, 2_048);
    assert_eq!(window_usage.compaction_buffer_tokens, 1_024);
    assert_eq!(window_usage.effective_prompt_budget, 1_024);
}

#[tokio::test]
async fn test_prompt_builder_drops_expanded_tool_catalog_before_compaction() {
    let mut config = PromptBuildConfig::default();
    config.prompt.window.reserved_output_tokens =
        talos_config::ReservedOutputPolicy::Fixed { tokens: 256 };
    config.prompt.window.compaction_buffer_tokens = 128;
    config.load_policy.include_session_summary = false;
    config.load_policy.include_recent_messages = false;

    let baseline = PromptBuilder::new()
        .agent("talos", "assistant")
        .config(config.clone())
        .model_info(ModelInfo {
            id: "baseline".to_string(),
            name: "baseline".to_string(),
            provider: "test".to_string(),
            context_window: 16_384,
            max_tokens: 256,
            reasoning: false,
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        })
        .user_input("hello")
        .assemble()
        .await
        .expect("baseline prompt should build");

    let effective_budget = baseline.diagnostics.total_estimated_tokens + 32;
    let registry = ToolRegistry::new();
    for index in 0..6 {
        registry.register(Arc::new(VerboseTool {
            name: match index {
                0 => "alpha_tool",
                1 => "beta_tool",
                2 => "gamma_tool",
                3 => "delta_tool",
                4 => "epsilon_tool",
                _ => "zeta_tool",
            },
            description: "Inspect repository structure, ownership boundaries, runtime behavior, test surfaces, deployment assumptions, and implementation drift across subsystems with detailed evidence requirements.",
        }));
    }

    let assembly = PromptBuilder::new()
        .agent("talos", "assistant")
        .tools(&registry)
        .config(config)
        .model_info(ModelInfo {
            id: "tight-window".to_string(),
            name: "tight-window".to_string(),
            provider: "test".to_string(),
            context_window: (effective_budget + 256 + 128) as u32,
            max_tokens: 256,
            reasoning: false,
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        })
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt should build");

    assert!(assembly.diagnostics.window_usage.within_effective_budget);
    assert!(assembly
        .diagnostics
        .dropped_sections
        .contains(&PromptSectionKind::ExpandedToolCatalog));
    assert!(assembly
        .dynamic_suffix_sections
        .iter()
        .all(|section| section.kind != PromptSectionKind::ExpandedToolCatalog));
    assert!(assembly
        .dynamic_suffix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::RuntimeSessionContext));
}

#[tokio::test]
async fn test_prompt_builder_normalizes_memory_prefix() {
    let memory = MemoryStore::new(std::env::temp_dir().join(Uuid::new_v4().to_string()));
    let timestamp = Utc::now();
    memory
        .append_turn(&TalosTurn {
            turn_id: "turn-1".to_string(),
            session_id: "s1".to_string(),
            seq: memory.next_turn_seq("s1").await.expect("seq should load"),
            timestamp,
            items: vec![
                TalosOutputItem::UserInput {
                    text: "hello world".to_string(),
                },
                TalosOutputItem::OutputText {
                    text: "ack".to_string(),
                },
            ],
            usage: TalosUsage::default(),
            stop_reason: StopReason::EndTurn,
        })
        .await
        .expect("append should succeed");

    let messages = PromptBuilder::new()
        .memory_store("s1", &memory)
        .user_input("next")
        .build()
        .await
        .expect("prompt build should succeed");
    let user_history = messages
        .iter()
        .find(|msg| msg.role == Role::User && msg.content != "next")
        .expect("should include user history message");
    assert_eq!(user_history.content, "hello world");
}

#[tokio::test]
async fn test_prompt_builder_creates_and_injects_bootstrap_files() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let project_root = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let memory = MemoryStore::new(workspace_dir.clone());
    std::fs::create_dir_all(&project_root).expect("project root should exist");
    std::fs::write(
        project_root.join("AGENTS.md"),
        "# AGENTS\n\n- Prefer crate roots before docs.\n",
    )
    .expect("agents file should write");

    let mut config = PromptBuildConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.load_policy.include_session_summary = false;
    config.load_policy.include_recent_messages = false;

    let messages = PromptBuilder::new()
        .agent("talos", "assistant")
        .memory_store("s1", &memory)
        .project_root(Some(project_root))
        .config(config)
        .user_input("bootstrap")
        .build()
        .await
        .expect("prompt build should succeed");

    for name in ["SOUL.md", "USER.md", "MEMORY.md"] {
        assert!(
            workspace_dir.join(name).exists(),
            "expected {} to exist",
            name
        );
    }
    assert!(!workspace_dir.join("AGENTS.md").exists());
    assert!(!workspace_dir.join("TOOLS.md").exists());
    assert!(!workspace_dir.join("BOOTSTRAP.md").exists());
    assert!(!workspace_dir.join("IDENTITY.md").exists());
    assert!(!workspace_dir.join("HEARTBEAT.md").exists());

    let project_context = messages
        .iter()
        .find(|msg| msg.content.contains("# Project Context"))
        .expect("should include project context");
    assert!(project_context
        .content
        .contains("## AGENTS.md (project root)"));
    assert!(project_context.content.contains("## SOUL.md (workspace)"));
    assert!(project_context.content.contains("## MEMORY.md (workspace)"));
}

#[tokio::test]
async fn test_prompt_builder_minimal_mode_filters_bootstrap_files() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let project_root = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let memory = MemoryStore::new(workspace_dir.clone());
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should exist");
    std::fs::create_dir_all(&project_root).expect("project root should exist");
    std::fs::write(
        project_root.join("AGENTS.md"),
        "# AGENTS\n\n- Project rule.\n",
    )
    .expect("agents file should write");

    let mut config = PromptBuildConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.load_policy.include_session_summary = false;
    config.load_policy.include_recent_messages = false;

    let messages = PromptBuilder::new()
        .mode(PromptMode::Minimal)
        .memory_store("s1", &memory)
        .project_root(Some(project_root))
        .config(config)
        .user_input("bootstrap")
        .build()
        .await
        .expect("prompt build should succeed");

    let project_context = messages
        .iter()
        .find(|msg| msg.content.contains("# Project Context"))
        .expect("should include project context");
    assert!(project_context
        .content
        .contains("## AGENTS.md (project root)"));
    assert!(project_context.content.contains("## SOUL.md (workspace)"));
    assert!(project_context.content.contains("## USER.md (workspace)"));
    assert!(project_context.content.contains("## MEMORY.md (workspace)"));
    assert!(!project_context
        .content
        .contains("## HEARTBEAT.md (workspace)"));
    assert!(!project_context
        .content
        .contains("## IDENTITY.md (workspace)"));
}

#[tokio::test]
async fn test_prompt_builder_falls_back_to_workspace_agents_when_project_agents_missing() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let project_root = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let memory = MemoryStore::new(workspace_dir.clone());
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should exist");
    std::fs::create_dir_all(&project_root).expect("project root should exist");
    std::fs::write(
        workspace_dir.join("AGENTS.md"),
        "# AGENTS\n\n- Workspace fallback rule.\n",
    )
    .expect("workspace agents file should write");

    let mut config = PromptBuildConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.load_policy.include_session_summary = false;
    config.load_policy.include_recent_messages = false;

    let assembly = PromptBuilder::new()
        .memory_store("s1", &memory)
        .project_root(Some(project_root))
        .config(config)
        .user_input("bootstrap")
        .assemble()
        .await
        .expect("prompt assembly should succeed");

    let project_context = assembly
        .dynamic_suffix_sections
        .iter()
        .find(|section| section.kind == PromptSectionKind::ProjectContext)
        .expect("project context should exist");
    assert!(project_context.content.contains("## AGENTS.md (workspace)"));
    assert!(project_context.content.contains("Workspace fallback rule"));
}

#[tokio::test]
async fn test_prompt_builder_full_mode_includes_bootstrap_extensions() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let project_root = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let memory = MemoryStore::new(workspace_dir.clone());
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should exist");
    std::fs::create_dir_all(&project_root).expect("project root should exist");
    std::fs::write(
        project_root.join("AGENTS.md"),
        "# AGENTS\n\n- Project rule.\n",
    )
    .expect("agents file should write");

    let mut config = PromptBuildConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.load_policy.include_session_summary = false;
    config.load_policy.include_recent_messages = false;
    std::fs::write(
        workspace_dir.join("IDENTITY.md"),
        "# IDENTITY\n\n- Override mission details.\n",
    )
    .expect("identity file should write");
    std::fs::write(
        workspace_dir.join("HEARTBEAT.md"),
        "# HEARTBEAT\n\n- Report major milestones.\n",
    )
    .expect("heartbeat file should write");

    let assembly = PromptBuilder::new()
        .mode(PromptMode::Full)
        .memory_store("s1", &memory)
        .project_root(Some(project_root))
        .config(config)
        .user_input("bootstrap")
        .assemble()
        .await
        .expect("prompt assembly should succeed");

    assert!(assembly
        .static_prefix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::IdentityCore));
    assert!(assembly
        .dynamic_suffix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::RuntimeSessionContext));
    let project_context = assembly
        .dynamic_suffix_sections
        .iter()
        .find(|section| section.kind == PromptSectionKind::ProjectContext)
        .expect("full mode should include project context");
    assert!(project_context
        .content
        .contains("## HEARTBEAT.md (workspace)"));
    assert!(project_context
        .content
        .contains("## IDENTITY.md (workspace)"));
    assert!(project_context
        .content
        .contains("## AGENTS.md (project root)"));
}

#[tokio::test]
async fn test_prompt_builder_includes_recent_task_continuity_section() {
    let assembly = PromptBuilder::new()
        .session_continuity(Some(SessionContinuity {
            recent_verified_paths: vec!["/tmp/project/Cargo.toml".to_string()],
            recent_successful_tools: vec!["Grep".to_string(), "Read".to_string()],
            recent_failed_tools: vec!["Bash".to_string()],
            recent_unhelpful_patterns: vec![
                "avoid retrying the same failing path or command".to_string()
            ],
            recent_answer_mode: Some(ContinuityAnswerMode::CodeFirst),
        }))
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt assembly should succeed");

    let continuity = assembly
        .dynamic_suffix_sections
        .iter()
        .find(|section| section.kind == PromptSectionKind::RecentTaskContinuity)
        .expect("continuity section should exist");
    assert!(continuity.content.contains("recent_successful_tools"));
    assert!(continuity.content.contains("code_first"));
}

#[tokio::test]
async fn test_prompt_builder_none_mode_skips_bootstrap_context() {
    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let memory = MemoryStore::new(workspace_dir.clone());

    let mut config = PromptBuildConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.load_policy.include_session_summary = false;
    config.load_policy.include_recent_messages = false;

    let assembly = PromptBuilder::new()
        .mode(PromptMode::None)
        .memory_store("s1", &memory)
        .config(config)
        .user_input("bootstrap")
        .assemble()
        .await
        .expect("prompt assembly should succeed");

    assert!(assembly
        .dynamic_suffix_sections
        .iter()
        .all(|section| section.kind != PromptSectionKind::ProjectContext));
    assert!(assembly.diagnostics.bootstrap_files.is_empty());
}

#[tokio::test]
async fn test_prompt_assembly_splits_static_prefix_and_dynamic_suffix_sections() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));

    let assembly = PromptBuilder::new()
        .agent("talos", "assistant")
        .tools(&registry)
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt assembly should succeed");

    assert!(assembly
        .static_prefix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::IdentityCore));
    assert!(assembly
        .static_prefix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::EvidenceToolingPolicy));
    assert!(assembly
        .static_prefix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::ToolChoicePriority));
    assert!(assembly
        .static_prefix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::FailureRecoveryPolicy));
    assert!(assembly
        .static_prefix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::CompactToolRouting));
    assert!(assembly
        .static_prefix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::TaskProtocolLibrary));
    assert!(assembly
        .static_prefix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::CommunicationPolicy));
    assert!(assembly
        .static_prefix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::RequestedToolPreferenceTemplate));
    assert!(assembly
        .dynamic_suffix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::RuntimeSessionContext));
    let tool_choice = assembly
        .static_prefix_sections
        .iter()
        .find(|section| section.kind == PromptSectionKind::ToolChoicePriority)
        .expect("tool choice priority should exist");
    assert!(tool_choice.content.contains("honor that tool"));
    assert!(tool_choice.content.contains("prefer Grep first"));
    let compact_tool_routing = assembly
        .static_prefix_sections
        .iter()
        .find(|section| section.kind == PromptSectionKind::CompactToolRouting)
        .expect("compact tool routing should exist");
    assert!(compact_tool_routing
        .content
        .contains("instead of shell grep or rg"));
    assert!(compact_tool_routing
        .content
        .contains("instead of cat, head, or tail"));
    let tooling = assembly
        .dynamic_suffix_sections
        .iter()
        .find(|section| section.kind == PromptSectionKind::ExpandedToolCatalog)
        .expect("tooling section should exist");
    assert!(tooling.content.contains("Available tools:"));
    assert!(tooling.content.contains("echo"));
    assert_eq!(tooling.cache_scope, CacheScope::None);
    assert!(!assembly.dynamic_suffix_sections.is_empty());
}

#[tokio::test]
async fn test_prompt_builder_splits_requested_tool_preference_template_and_hint() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(GrepTool));

    let assembly = PromptBuilder::new()
        .agent("talos", "assistant")
        .tools(&registry)
        .user_input("Analyze the repository architecture and use Grep first.")
        .assemble()
        .await
        .expect("prompt assembly should succeed");

    let requested_tool_preference_template = assembly
        .static_prefix_sections
        .iter()
        .find(|section| section.kind == PromptSectionKind::RequestedToolPreferenceTemplate)
        .expect("requested tool preference template should exist");
    assert!(requested_tool_preference_template
        .content
        .contains("explicitly names a tool"));

    let requested_tool_preference_hint = assembly
        .dynamic_suffix_sections
        .iter()
        .find(|section| section.kind == PromptSectionKind::RequestedToolPreferenceHint)
        .expect("requested tool preference hint should exist");
    assert!(requested_tool_preference_hint.content.contains("Grep"));
    assert!(requested_tool_preference_hint
        .content
        .contains("start with Grep rather than Glob"));
}

#[tokio::test]
async fn test_rendered_cache_breakpoints_stay_before_user_input() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));

    let assembly = PromptBuilder::new()
        .agent("talos", "assistant")
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
    assert!(rendered
        .iter()
        .take(last_cached_index + 1)
        .all(|message| message.role == Role::System));
}

#[tokio::test]
async fn test_rendered_cache_breakpoints_cover_static_prefix_and_workspace_sections() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool));

    let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
    let memory = MemoryStore::new(workspace_dir.clone());

    let mut config = PromptBuildConfig::default();
    config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
    config.load_policy.include_session_summary = false;
    config.load_policy.include_recent_messages = false;

    let assembly = PromptBuilder::new()
        .agent("talos", "assistant")
        .tools(&registry)
        .memory_store("s1", &memory)
        .config(config)
        .user_input("hello")
        .assemble()
        .await
        .expect("prompt assembly should succeed");

    let rendered = render_prompt_assembly(&assembly);
    let project_context_index = assembly.static_prefix_sections.len()
        + assembly
            .dynamic_suffix_sections
            .iter()
            .position(|section| section.kind == PromptSectionKind::ProjectContext)
            .expect("project context should exist");
    assert!(rendered[project_context_index].cache_control.is_some());
    assert!(assembly
        .dynamic_suffix_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::ProjectContext));
    assert!(rendered
        .iter()
        .take(rendered.len() - 1)
        .any(|message| message.cache_control.is_some()));
}
