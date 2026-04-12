use crate::template::PromptTemplate;

/// Default system message for assistants.
pub fn default_system_prompt() -> PromptTemplate {
    PromptTemplate::new(
        r#"You are a helpful AI assistant. Respond concisely and accurately to user queries.
Context: {{context}}
Previous conversation: {{history}}"#
            .to_string(),
        "Default system prompt for AI assistants".to_string(),
    )
}

/// Tool usage prompt template.
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

/// Memory recall prompt template.
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

/// Agent persona prompt template.
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
