//! Talos Prompt - Prompt Template Engine
//!
//! This module provides a flexible prompt templating system that enables:
//! - Template definition and rendering
//! - Context-aware variable substitution
//! - Predefined prompt templates for common use cases
//! - Dynamic prompt assembly based on context

mod assembler;
mod bootstrap;
mod budget;
mod diagnostics;
mod renderer;
mod types;

pub use types::{
    BootstrapFileDiagnostic, BudgetTier, CacheScope, PlannerMode, PromptAssembly, PromptBudget,
    PromptBuildConfig, PromptDiagnostics, PromptLoadPolicy, PromptMode, PromptPhase, PromptSection,
    PromptSectionKind, TruncationMode, WorkspaceBootstrapFile, WorkspaceBootstrapFileKind,
};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use talos_error::TalosResult;
use talos_llm_provider::ChatMessage;
use talos_memory::{MemoryStore, RetrievedMemoryItem};
use talos_tools::ToolDefinitionProvider;
use talos_types::talos::TalosSessionEnvironment;
use tokio::fs;

/// A prompt template that can be rendered with context variables
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTemplate {
    /// The raw template string with placeholders
    pub template: String,
    /// Optional description of the template's purpose
    pub description: String,
    /// List of required variables for this template
    pub required_vars: Vec<String>,
}

impl PromptTemplate {
    /// Create a new prompt template
    pub fn new(template: impl Into<String>, description: impl Into<String>) -> Self {
        let template_str = template.into();
        let required_vars = Self::extract_variables(&template_str);

        Self {
            template: template_str,
            description: description.into(),
            required_vars,
        }
    }

    /// Create a new prompt template with explicit required variables
    pub fn new_with_vars(
        template: impl Into<String>,
        description: impl Into<String>,
        required_vars: Vec<String>,
    ) -> Self {
        Self {
            template: template.into(),
            description: description.into(),
            required_vars,
        }
    }

    /// Render the template with provided context
    pub fn render(&self, context: &HashMap<String, String>) -> TalosResult<String> {
        let mut result = self.template.clone();

        // Check for missing required variables
        for var in &self.required_vars {
            if !context.contains_key(var) {
                return Err(talos_error::TalosError::ValidationError(format!(
                    "Missing required variable '{}' for template: {}",
                    var, self.description
                )));
            }
        }

        // Replace variables in the template
        for (key, value) in context {
            let placeholder = format!("{{{{{}}}}}", key); // {{key}} format
            result = result.replace(&placeholder, value);
        }

        Ok(result)
    }

    /// Extract variable names from template string
    /// Looks for {{variable_name}} format
    fn extract_variables(template: &str) -> Vec<String> {
        let mut vars = Vec::new();
        let chars: Vec<char> = template.chars().collect();

        let mut i = 0;
        while i < chars.len() {
            if i + 3 < chars.len() && chars[i] == '{' && chars[i + 1] == '{' && chars[i + 2] != '{'
            {
                // Found {{
                let start = i + 2;
                let mut end = start;

                // Look for }}
                while end < chars.len() {
                    if end + 1 < chars.len() && chars[end] == '}' && chars[end + 1] == '}' {
                        break;
                    }
                    end += 1;
                }

                if end < chars.len() {
                    // Extract variable name and trim whitespace
                    let var_name: String = chars[start..end].iter().collect();
                    let var_name = var_name.trim().to_string();

                    if !var_name.is_empty() && !vars.contains(&var_name) {
                        vars.push(var_name);
                    }

                    i = end + 2; // Skip }}
                    continue;
                }
            }

            i += 1;
        }

        vars
    }

    /// Render with default values for missing variables
    pub fn render_with_defaults(
        &self,
        context: &HashMap<String, String>,
        defaults: &HashMap<String, String>,
    ) -> TalosResult<String> {
        let mut full_context = defaults.clone();
        full_context.extend(context.clone());

        self.render(&full_context)
    }
}

/// Manager for multiple prompt templates
pub struct PromptManager {
    templates: HashMap<String, Arc<PromptTemplate>>,
}

impl PromptManager {
    /// Create a new prompt manager
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    /// Register a new template
    pub fn register_template(&mut self, name: impl Into<String>, template: PromptTemplate) {
        self.templates.insert(name.into(), Arc::new(template));
    }

    /// Get a template by name
    pub fn get_template(&self, name: &str) -> Option<&Arc<PromptTemplate>> {
        self.templates.get(name)
    }

    /// Render a named template with context
    pub fn render_template(
        &self,
        name: &str,
        context: &HashMap<String, String>,
    ) -> TalosResult<String> {
        match self.get_template(name) {
            Some(template) => template.render(context),
            None => Err(talos_error::TalosError::ValidationError(format!(
                "Template '{}' not found",
                name
            ))),
        }
    }

    /// Check if a template exists
    pub fn has_template(&self, name: &str) -> bool {
        self.templates.contains_key(name)
    }

    /// List all registered template names
    pub fn list_templates(&self) -> Vec<String> {
        self.templates.keys().cloned().collect()
    }
}

/// Predefined system prompt templates
pub mod presets {
    use super::*;

    /// Default system message for assistants
    pub fn default_system_prompt() -> PromptTemplate {
        PromptTemplate::new(
            r#"You are a helpful AI assistant. Respond concisely and accurately to user queries.
Context: {{context}}
Previous conversation: {{history}}"#
                .to_string(),
            "Default system prompt for AI assistants".to_string(),
        )
    }

    /// Tool usage prompt template
    pub fn tool_usage_prompt() -> PromptTemplate {
        PromptTemplate::new_with_vars(
            r#"You have access to the following tools:

{{available_tools}}

To use a tool, respond with a JSON object in the format:
{"tool_name": "tool_name", "arguments": {"arg1": "value1"}}

Current conversation history:
{{conversation_history}}

User: {{user_input}}

Assistant:"#,
            "Prompt template for tool usage".to_string(),
            vec![
                "available_tools".to_string(),
                "conversation_history".to_string(),
                "user_input".to_string(),
            ],
        )
    }

    /// Memory recall prompt template
    pub fn memory_recall_prompt() -> PromptTemplate {
        PromptTemplate::new(
            r#"Based on the following memories, please respond to the user's query:

Relevant memories:
{{relevant_memories}}

User query: {{user_query}}

Please provide a response that incorporates information from the relevant memories where appropriate."#,
            "Prompt template for memory recall".to_string(),
        )
    }

    /// Agent persona prompt template
    pub fn agent_persona_prompt() -> PromptTemplate {
        PromptTemplate::new(
            r#"{{persona_description}}

Your current task is: {{current_task}}

Context information:
{{context_info}}

Please respond appropriately based on your persona and the current task."#,
            "Prompt template for agent persona".to_string(),
        )
    }
}

/// Context builder for assembling prompt contexts
pub struct PromptContext {
    pub variables: HashMap<String, String>,
}

impl PromptContext {
    /// Create a new context
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
        }
    }

    /// Add a variable to the context
    pub fn add<V: ToString>(mut self, key: impl Into<String>, value: V) -> Self {
        self.variables.insert(key.into(), value.to_string());
        self
    }

    /// Extend context with multiple variables
    pub fn extend(mut self, other: HashMap<String, String>) -> Self {
        self.variables.extend(other);
        self
    }

    /// Get a variable value
    pub fn get(&self, key: &str) -> Option<&String> {
        self.variables.get(key)
    }

    /// Check if a variable exists
    pub fn contains_key(&self, key: &str) -> bool {
        self.variables.contains_key(key)
    }
}

impl Default for PromptContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Unified prompt builder for kernel turns.
pub struct PromptBuilder<'a> {
    pub(crate) agent_name: String,
    pub(crate) agent_description: String,
    pub(crate) mode: PromptMode,
    pub(crate) phase: PromptPhase,
    pub(crate) planner_mode: PlannerMode,
    pub(crate) planning_instruction: Option<String>,
    pub(crate) user_input: Option<String>,
    pub(crate) tool_registry: Option<&'a dyn ToolDefinitionProvider>,
    pub(crate) memory_store: Option<&'a MemoryStore>,
    pub(crate) session_id: Option<&'a str>,
    pub(crate) retrieved_memories: Vec<RetrievedMemoryItem>,
    pub(crate) session_environment: Option<TalosSessionEnvironment>,
    pub(crate) config: PromptBuildConfig,
}

impl<'a> Default for PromptBuilder<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> PromptBuilder<'a> {
    pub fn new() -> Self {
        Self {
            agent_name: "talos".to_string(),
            agent_description: "AI assistant".to_string(),
            mode: PromptMode::Full,
            phase: PromptPhase::Execution,
            planner_mode: PlannerMode::BuiltIn,
            planning_instruction: None,
            user_input: None,
            tool_registry: None,
            memory_store: None,
            session_id: None,
            retrieved_memories: Vec::new(),
            session_environment: None,
            config: PromptBuildConfig::default(),
        }
    }

    pub fn agent(mut self, name: impl Into<String>, description: impl Into<String>) -> Self {
        self.agent_name = name.into();
        self.agent_description = description.into();
        self
    }

    pub fn phase(mut self, phase: PromptPhase) -> Self {
        self.phase = phase;
        self
    }

    pub fn mode(mut self, mode: PromptMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn planner_mode(mut self, planner_mode: impl Into<String>) -> TalosResult<Self> {
        self.planner_mode = PlannerMode::parse(&planner_mode.into())?;
        Ok(self)
    }

    pub fn planning_instruction(mut self, planning_instruction: Option<String>) -> Self {
        self.planning_instruction = planning_instruction;
        self
    }

    pub fn tools(mut self, tool_registry: &'a dyn ToolDefinitionProvider) -> Self {
        self.tool_registry = Some(tool_registry);
        self
    }

    pub fn memory_store(mut self, session_id: &'a str, memory_store: &'a MemoryStore) -> Self {
        self.session_id = Some(session_id);
        self.memory_store = Some(memory_store);
        self
    }

    pub fn user_input(mut self, input: impl Into<String>) -> Self {
        self.user_input = Some(input.into());
        self
    }

    pub fn config(mut self, config: PromptBuildConfig) -> Self {
        self.config = config;
        self
    }

    pub fn retrieved_memories(mut self, memories: Vec<RetrievedMemoryItem>) -> Self {
        self.retrieved_memories = memories;
        self
    }

    pub fn session_environment(mut self, environment: Option<TalosSessionEnvironment>) -> Self {
        self.session_environment = environment;
        self
    }

    pub fn budget(mut self, max_static_tokens: usize, max_dynamic_tokens: usize) -> Self {
        self.config.budget.max_static_tokens = max_static_tokens.max(1);
        self.config.budget.max_dynamic_tokens = max_dynamic_tokens.max(1);
        self
    }

    pub fn include_session_summary(mut self, include: bool) -> Self {
        self.config.load_policy.include_session_summary = include;
        self
    }

    pub fn include_recent_messages(mut self, include: bool) -> Self {
        self.config.load_policy.include_recent_messages = include;
        self
    }

    pub async fn build(self) -> TalosResult<Vec<ChatMessage>> {
        let assembly = self.assemble().await?;
        Ok(renderer::render_assembly(&assembly))
    }

    pub async fn assemble(self) -> TalosResult<PromptAssembly> {
        assembler::assemble(&self).await
    }

    fn render_tool_summary(tool_registry: &dyn ToolDefinitionProvider) -> String {
        let mut definitions = tool_registry.get_tool_definitions();
        if definitions.is_empty() {
            return "none".to_string();
        }

        definitions.sort_by(|a, b| a.name.cmp(&b.name));
        definitions
            .into_iter()
            .map(|tool| {
                let desc = tool.description.trim();
                if desc.is_empty() {
                    format!("- {}", tool.name)
                } else {
                    format!("- {}: {}", tool.name, desc)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) async fn load_workspace_bootstrap_files(
        &self,
    ) -> TalosResult<Vec<WorkspaceBootstrapFile>> {
        if !self.config.prompt.bootstrap.enabled {
            return Ok(Vec::new());
        }
        let workspace_dir = bootstrap::expand_workspace_dir(&self.config.memory.workspace_dir);
        fs::create_dir_all(&workspace_dir).await.map_err(|err| {
            talos_error::TalosError::StorageError(format!(
                "create prompt workspace dir {}: {}",
                workspace_dir.display(),
                err
            ))
        })?;

        let mut files = Vec::new();
        for kind in WorkspaceBootstrapFileKind::all() {
            if !kind.should_load_in_mode(self.mode, &self.config.prompt) {
                continue;
            }
            let path = workspace_dir.join(kind.file_name());
            if self.config.prompt.bootstrap.auto_create_missing_files {
                bootstrap::ensure_bootstrap_file(&path, kind.default_template()).await?;
            }
            let content = match fs::read_to_string(&path).await {
                Ok(content) => Some(content),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
                Err(err) => {
                    return Err(talos_error::TalosError::StorageError(format!(
                        "read bootstrap file {}: {}",
                        path.display(),
                        err
                    )));
                }
            };
            files.push(WorkspaceBootstrapFile {
                kind: *kind,
                path,
                content,
            });
        }
        Ok(files)
    }
}

pub fn render_prompt_assembly(assembly: &PromptAssembly) -> Vec<ChatMessage> {
    renderer::render_assembly(assembly)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use serde_json::Value;
    use talos_llm_provider::Role;
    use talos_memory::{
        Memory, MemoryStore, RecordHeader, TalosMessage, TalosMessageRole, TalosTurn,
    };
    use talos_tools::{Tool, ToolContext, ToolOutput, ToolRegistry};
    use uuid::Uuid;

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

        let mut context = HashMap::new();
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

        let mut context = HashMap::new();
        context.insert("name".to_string(), "Alice".to_string());
        // Missing "role"

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

        let mut context = HashMap::new();
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

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "Echo input for testing"
        }

        async fn execute(&self, _input: Value, _context: ToolContext) -> TalosResult<ToolOutput> {
            Ok(ToolOutput {
                success: true,
                output: "ok".to_string(),
                error: None,
            })
        }
    }

    #[tokio::test]
    async fn test_prompt_builder_basic() {
        let messages = PromptBuilder::new()
            .agent("talos", "assistant")
            .phase(PromptPhase::Execution)
            .user_input("hello")
            .build()
            .await
            .expect("prompt build should succeed");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::System);
        assert_eq!(messages[1].role, Role::User);
        assert_eq!(messages[1].content, "hello");
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
                    messages: vec![TalosMessage {
                        header: RecordHeader {
                            id: format!("{turn_id}:a"),
                            session_id: "s1".to_string(),
                            turn_id: Some(turn_id),
                            timestamp,
                        },
                        role: TalosMessageRole::Assistant,
                        text: format!("historical message {}", idx),
                    }],
                    reasoning: Vec::new(),
                    tool_calls: Vec::new(),
                    tool_results: Vec::new(),
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
        let messages = PromptBuilder::new()
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
            .build()
            .await
            .expect("prompt build should succeed");

        assert!(messages.len() >= 2);
        assert!(messages
            .iter()
            .any(|message| message.content.contains("[truncated]")));
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
                messages: vec![TalosMessage {
                    header: RecordHeader {
                        id: "turn-1:u".to_string(),
                        session_id: "s1".to_string(),
                        turn_id: Some("turn-1".to_string()),
                        timestamp,
                    },
                    role: TalosMessageRole::User,
                    text: "hello world".to_string(),
                }],
                reasoning: Vec::new(),
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
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
        let memory = MemoryStore::new(workspace_dir.clone());

        let mut config = PromptBuildConfig::default();
        config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
        config.load_policy.include_session_summary = false;
        config.load_policy.include_recent_messages = false;

        let messages = PromptBuilder::new()
            .agent("talos", "assistant")
            .memory_store("s1", &memory)
            .config(config)
            .user_input("bootstrap")
            .build()
            .await
            .expect("prompt build should succeed");

        for name in [
            "AGENTS.md",
            "SOUL.md",
            "TOOLS.md",
            "IDENTITY.md",
            "USER.md",
            "HEARTBEAT.md",
            "BOOTSTRAP.md",
            "MEMORY.md",
        ] {
            assert!(
                workspace_dir.join(name).exists(),
                "expected {} to exist",
                name
            );
        }

        let project_context = messages
            .iter()
            .find(|msg| msg.content.contains("# Project Context"))
            .expect("should include project context");
        assert!(project_context.content.contains("## AGENTS.md"));
        assert!(project_context.content.contains("## SOUL.md"));
        assert!(project_context.content.contains("## MEMORY.md"));
    }

    #[tokio::test]
    async fn test_prompt_builder_minimal_mode_filters_bootstrap_files() {
        let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let memory = MemoryStore::new(workspace_dir.clone());

        let mut config = PromptBuildConfig::default();
        config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
        config.load_policy.include_session_summary = false;
        config.load_policy.include_recent_messages = false;

        let messages = PromptBuilder::new()
            .mode(PromptMode::Minimal)
            .memory_store("s1", &memory)
            .config(config)
            .user_input("bootstrap")
            .build()
            .await
            .expect("prompt build should succeed");

        let project_context = messages
            .iter()
            .find(|msg| msg.content.contains("# Project Context"))
            .expect("should include project context");
        assert!(project_context.content.contains("## AGENTS.md"));
        assert!(project_context.content.contains("## SOUL.md"));
        assert!(!project_context.content.contains("## HEARTBEAT.md"));
        assert!(!project_context.content.contains("## BOOTSTRAP.md"));
    }

    #[tokio::test]
    async fn test_prompt_builder_full_mode_includes_bootstrap_extensions() {
        let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let memory = MemoryStore::new(workspace_dir.clone());

        let mut config = PromptBuildConfig::default();
        config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
        config.load_policy.include_session_summary = false;
        config.load_policy.include_recent_messages = false;

        let assembly = PromptBuilder::new()
            .mode(PromptMode::Full)
            .memory_store("s1", &memory)
            .config(config)
            .user_input("bootstrap")
            .assemble()
            .await
            .expect("prompt assembly should succeed");

        assert!(assembly
            .system_sections
            .iter()
            .any(|section| section.kind == PromptSectionKind::System));
        let project_context = assembly
            .context_sections
            .iter()
            .find(|section| section.kind == PromptSectionKind::ProjectContext)
            .expect("full mode should include project context");
        assert!(project_context.content.contains("## HEARTBEAT.md"));
        assert!(project_context.content.contains("## BOOTSTRAP.md"));
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
            .any(|section| section.kind == PromptSectionKind::System));
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
}
