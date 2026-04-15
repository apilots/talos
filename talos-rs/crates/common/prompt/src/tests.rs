use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use talos_llm_provider::Role;
use talos_memory::{Memory, MemoryStore, TalosTurn};
use talos_tools::{Tool, ToolContext, ToolOutput, ToolRegistry};
use talos_types::talos::{StopReason, TalosOutputItem, TalosUsage};
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

    async fn execute(
        &self,
        _input: Value,
        _context: ToolContext,
    ) -> talos_error::TalosResult<ToolOutput> {
        Ok(ToolOutput {
            success: true,
            output: "ok".to_string(),
            error: None,
        })
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

    assert!(assembly.system_sections.len() >= 4);
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
async fn test_prompt_builder_dynamic_budget_trims_history() {
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

    let messages = PromptBuilder::new()
        .agent("talos", "assistant")
        .memory_store("s1", &memory)
        .config(PromptBuildConfig {
            budget: PromptBudget {
                max_static_tokens: 1200,
                max_dynamic_tokens: 8,
            },
            ..PromptBuildConfig::default()
        })
        .user_input("latest")
        .build()
        .await
        .expect("prompt build should succeed");

    let dynamic_count = messages
        .iter()
        .filter(|msg| msg.role != Role::System && msg.role != Role::User)
        .count();
    assert!(dynamic_count <= 1);
    assert_eq!(
        messages.last().expect("must have user message").role,
        Role::User
    );
}

#[tokio::test]
async fn test_prompt_builder_uses_explicit_budget_configuration() {
    let assembly = PromptBuilder::new()
        .agent("talos", "assistant")
        .planning_instruction(Some("keep it short".to_string()))
        .config(PromptBuildConfig {
            budget: PromptBudget {
                max_static_tokens: 12,
                max_dynamic_tokens: 3200,
            },
            ..PromptBuildConfig::default()
        })
        .user_input("latest")
        .assemble()
        .await
        .expect("prompt build should succeed");

    assert!(
        !assembly.diagnostics.truncated_sections.is_empty()
            || !assembly.diagnostics.dropped_sections.is_empty()
    );
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
        .system_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::IdentityMission));
    let project_context = assembly
        .context_sections
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
        .context_sections
        .iter()
        .all(|section| section.kind != PromptSectionKind::ProjectContext));
    assert!(assembly.diagnostics.bootstrap_files.is_empty());
}

#[tokio::test]
async fn test_prompt_assembly_splits_system_and_tooling_sections() {
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
        .system_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::IdentityMission));
    assert!(assembly
        .system_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::EvidenceToolingPolicy));
    assert!(assembly
        .system_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::TaskProtocolLibrary));
    assert!(assembly
        .system_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::CommunicationPolicy));
    let tooling = assembly
        .system_sections
        .iter()
        .find(|section| section.kind == PromptSectionKind::Tooling)
        .expect("tooling section should exist");
    assert!(tooling.content.contains("Available tools:"));
    assert!(tooling.content.contains("echo"));
    assert_eq!(tooling.cache_scope, CacheScope::Static);
    assert!(assembly.context_sections.is_empty());
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
async fn test_rendered_cache_breakpoints_cover_tooling_and_workspace_sections() {
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
    let tooling_index = assembly
        .system_sections
        .iter()
        .position(|section| section.kind == PromptSectionKind::Tooling)
        .expect("tooling section should exist");
    assert!(rendered[tooling_index].cache_control.is_some());
    assert!(assembly
        .context_sections
        .iter()
        .any(|section| section.kind == PromptSectionKind::ProjectContext));
    assert!(rendered
        .iter()
        .take(rendered.len() - 1)
        .any(|message| message.cache_control.is_some()));
}
