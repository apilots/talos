//! Talos Prompt - Prompt Template Engine
//!
//! This module provides a flexible prompt templating system that enables:
//! - Template definition and rendering
//! - Context-aware variable substitution
//! - Predefined prompt templates for common use cases
//! - Dynamic prompt assembly based on context

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use talos_config::{AgentKernelMemoryConfig, AgentKernelPromptConfig};
use talos_error::TalosResult;
use talos_llm_provider::ChatMessage;
use talos_memory::{
    Memory, MemoryStore, RetrievedMemoryItem, SessionPromptContextConfig, TalosMessageRole,
};
use talos_tools::ToolRegistry;

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

/// Prompt build phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PromptPhase {
    Planning,
    #[default]
    Execution,
}

/// Prompt budget controls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptBudget {
    pub max_static_tokens: usize,
    pub max_dynamic_tokens: usize,
}

impl Default for PromptBudget {
    fn default() -> Self {
        Self {
            max_static_tokens: 1200,
            max_dynamic_tokens: 3200,
        }
    }
}

/// Prompt context loading policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptLoadPolicy {
    pub include_session_summary: bool,
    pub include_recent_messages: bool,
    pub include_root_memory: bool,
    pub include_retrieved_memories: bool,
    pub session_recent_limit: usize,
    pub retrieved_memory_max_items: usize,
    pub retrieved_memory_max_chars: usize,
    pub unified_recent_limit: usize,
}

impl Default for PromptLoadPolicy {
    fn default() -> Self {
        Self {
            include_session_summary: true,
            include_recent_messages: true,
            include_root_memory: true,
            include_retrieved_memories: true,
            session_recent_limit: 16,
            retrieved_memory_max_items: 4,
            retrieved_memory_max_chars: 2000,
            unified_recent_limit: 10,
        }
    }
}

/// Prompt build configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptBuildConfig {
    pub budget: PromptBudget,
    pub load_policy: PromptLoadPolicy,
    pub prompt: AgentKernelPromptConfig,
    pub memory: AgentKernelMemoryConfig,
}

/// Unified prompt builder for kernel turns.
pub struct PromptBuilder<'a> {
    agent_name: String,
    agent_description: String,
    phase: PromptPhase,
    planner_mode: String,
    planning_instruction: Option<String>,
    user_input: Option<String>,
    tool_registry: Option<&'a ToolRegistry>,
    memory_store: Option<&'a MemoryStore>,
    session_id: Option<&'a str>,
    retrieved_memories: Vec<RetrievedMemoryItem>,
    config: PromptBuildConfig,
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
            phase: PromptPhase::Execution,
            planner_mode: "built_in".to_string(),
            planning_instruction: None,
            user_input: None,
            tool_registry: None,
            memory_store: None,
            session_id: None,
            retrieved_memories: Vec::new(),
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

    pub fn planner_mode(mut self, planner_mode: impl Into<String>) -> Self {
        self.planner_mode = planner_mode.into();
        self
    }

    pub fn planning_instruction(mut self, planning_instruction: Option<String>) -> Self {
        self.planning_instruction = planning_instruction;
        self
    }

    pub fn tools(mut self, tool_registry: &'a ToolRegistry) -> Self {
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
        let user_input = self.user_input.clone().ok_or_else(|| {
            talos_error::TalosError::ValidationError(
                "PromptBuilder requires user input".to_string(),
            )
        })?;

        let mut static_messages = vec![ChatMessage::system(self.build_identity_prompt())];
        if let Some(planning_instruction) = self.planning_instruction.as_deref() {
            if !planning_instruction.trim().is_empty() {
                static_messages.push(ChatMessage::system(format!(
                    "Planning instruction:\n{}",
                    planning_instruction.trim()
                )));
            }
        }
        self.enforce_static_budget(&mut static_messages);

        let mut dynamic_messages = self.load_dynamic_messages().await?;
        self.enforce_dynamic_budget(&mut dynamic_messages);

        let mut messages = Vec::with_capacity(static_messages.len() + dynamic_messages.len() + 1);
        messages.extend(static_messages);
        messages.extend(dynamic_messages);
        messages.push(ChatMessage::user(user_input));
        Ok(messages)
    }

    fn build_identity_prompt(&self) -> String {
        let phase_label = match self.phase {
            PromptPhase::Planning => "planning",
            PromptPhase::Execution => "execution",
        };
        let phase_instruction = match self.phase {
            PromptPhase::Planning => {
                "Focus on clear decomposition, constraints, and an executable next-step plan."
            }
            PromptPhase::Execution => {
                "Use native tool calling when needed, then produce the final response."
            }
        };
        let planner_mode = self.planner_mode.trim().to_ascii_lowercase();

        let tool_summary = self
            .tool_registry
            .map(Self::render_tool_summary)
            .unwrap_or_else(|| "none".to_string());

        format!(
            r#"You are {}, {}

Phase: {}
Planner mode: {}
{}

Available tools:
{}"#,
            self.agent_name,
            self.agent_description,
            phase_label,
            planner_mode,
            phase_instruction,
            tool_summary
        )
    }

    fn render_tool_summary(tool_registry: &ToolRegistry) -> String {
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

    async fn load_dynamic_messages(&self) -> TalosResult<Vec<ChatMessage>> {
        let mut messages = Vec::new();
        let include_summary = self.config.load_policy.include_session_summary;
        let include_recent = self.config.load_policy.include_recent_messages;
        let include_root_memory = self.config.load_policy.include_root_memory;
        let include_retrieved_memories = self.config.load_policy.include_retrieved_memories;
        let summary_recent_count = self.config.memory.session_summary_recent_count;
        let summary_max_chars = self.config.memory.session_summary_max_chars;
        let context_target_messages = self.config.load_policy.session_recent_limit;
        let memory_enable_summary = self.config.memory.session_summary_enabled;
        let retrieved_memory_max_items = self.config.load_policy.retrieved_memory_max_items;
        let retrieved_memory_max_chars = self.config.load_policy.retrieved_memory_max_chars;

        if let (Some(memory_store), Some(session_id)) = (self.memory_store, self.session_id) {
            let context = memory_store
                .build_session_prompt_context(
                    session_id,
                    &SessionPromptContextConfig {
                        recent_messages_limit: if include_recent {
                            context_target_messages
                        } else {
                            0
                        },
                        include_summary: include_summary && memory_enable_summary,
                        summary_recent_count,
                        summary_max_chars,
                    },
                )
                .await?;
            if include_summary {
                for summary in context.summaries {
                    let content = summary.content.trim().to_string();
                    if !content.is_empty() {
                        messages.push(ChatMessage::system(format!(
                            "Session memory summary (id={}):\n{}",
                            summary.header.id, content
                        )));
                    }
                }
            }
            if include_recent {
                for message in context.recent_messages {
                    messages.push(Self::talos_message_to_chat_message(
                        message.role,
                        message.text,
                    ));
                }
            }
            if include_root_memory {
                let root_memory = memory_store.load_root_memory().await?;
                if let Some(content) = root_memory.content {
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        messages.push(ChatMessage::system(format!(
                            "Root memory ({}):\n{}",
                            root_memory
                                .path
                                .file_name()
                                .and_then(|value| value.to_str())
                                .unwrap_or("MEMORY.md"),
                            trimmed
                        )));
                    }
                }
            }
            if include_retrieved_memories && !self.retrieved_memories.is_empty() {
                let mut combined = String::new();
                for item in self
                    .retrieved_memories
                    .iter()
                    .take(retrieved_memory_max_items.max(1))
                {
                    let line = format!(
                        "- {}{}: {}",
                        item.path,
                        item.citation
                            .as_ref()
                            .map(|citation| format!(" ({citation})"))
                            .unwrap_or_default(),
                        item.snippet.trim()
                    );
                    if combined.len() + line.len() + 1 > retrieved_memory_max_chars.max(1) {
                        break;
                    }
                    if !combined.is_empty() {
                        combined.push('\n');
                    }
                    combined.push_str(&line);
                }
                if !combined.is_empty() {
                    messages.push(ChatMessage::system(format!(
                        "Retrieved memories:\n{}",
                        combined
                    )));
                }
            }
            return Ok(messages);
        }

        Ok(messages)
    }

    fn talos_message_to_chat_message(role: TalosMessageRole, text: String) -> ChatMessage {
        match role {
            TalosMessageRole::User => ChatMessage::user(text),
            TalosMessageRole::Assistant => ChatMessage::assistant(text),
            TalosMessageRole::System => ChatMessage::system(text),
        }
    }

    fn enforce_static_budget(&self, messages: &mut Vec<ChatMessage>) {
        let budget = self.config.budget.max_static_tokens.max(1);
        Self::truncate_messages_to_budget(messages, budget, true);
    }

    fn enforce_dynamic_budget(&self, messages: &mut Vec<ChatMessage>) {
        let budget = self.config.budget.max_dynamic_tokens.max(1);
        Self::truncate_messages_to_budget(messages, budget, false);
    }

    fn truncate_messages_to_budget(
        messages: &mut Vec<ChatMessage>,
        budget: usize,
        preserve_first: bool,
    ) {
        if messages.is_empty() {
            return;
        }

        while Self::estimate_messages_tokens(messages) > budget {
            if preserve_first {
                if messages.len() > 1 {
                    let _ = messages.pop();
                    continue;
                }
            } else if !messages.is_empty() {
                messages.remove(0);
                continue;
            }

            let current = Self::estimate_messages_tokens(messages);
            if current <= budget {
                break;
            }

            let overflow = current.saturating_sub(budget);
            if let Some(last) = messages.last_mut() {
                let keep_tokens =
                    Self::estimate_text_tokens(&last.content).saturating_sub(overflow);
                last.content = Self::truncate_text_to_tokens(&last.content, keep_tokens.max(1));
            } else {
                break;
            }
        }
    }

    fn estimate_messages_tokens(messages: &[ChatMessage]) -> usize {
        messages
            .iter()
            .map(|message| Self::estimate_text_tokens(&message.content) + 4)
            .sum()
    }

    fn estimate_text_tokens(text: &str) -> usize {
        let chars = text.chars().count();
        if chars == 0 {
            0
        } else {
            (chars + 3) / 4
        }
    }

    fn truncate_text_to_tokens(text: &str, max_tokens: usize) -> String {
        let max_chars = max_tokens.saturating_mul(4);
        if text.chars().count() <= max_chars {
            return text.to_string();
        }

        const SUFFIX: &str = "...[truncated]";
        if max_chars <= SUFFIX.len() {
            return SUFFIX.chars().take(max_chars).collect();
        }

        let keep_chars = max_chars - SUFFIX.len();
        let mut out = text.chars().take(keep_chars).collect::<String>();
        out.push_str(SUFFIX);
        out
    }
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
    use talos_tools::{Tool, ToolContext, ToolOutput};
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
            .planning_instruction(Some("build a short plan first".to_string()))
            .tools(&registry)
            .user_input("help me")
            .build()
            .await
            .expect("prompt build should succeed");

        assert!(messages[0].content.contains("echo"));
        assert!(messages[1].content.contains("Planning instruction"));
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
}
