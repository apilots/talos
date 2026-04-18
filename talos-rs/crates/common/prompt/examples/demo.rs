//! Example demonstrating the usage of the Talos Prompt Template System

use std::collections::HashMap;
use talos_prompt::{presets, PromptContext, PromptManager, PromptTemplate};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Talos Prompt Template System Demo ===\n");

    // Example 1: Basic template usage
    println!("1. Basic Template Usage:");
    let greeting_template = PromptTemplate::new(
        "Hello, {{name}}! You are a {{role}} specializing in {{domain}}.".to_string(),
        "Simple greeting template".to_string(),
    );

    let mut context = HashMap::new();
    context.insert("name".to_string(), "Alice".to_string());
    context.insert("role".to_string(), "Senior Developer".to_string());
    context.insert("domain".to_string(), "AI Systems".to_string());

    let result = greeting_template.render(&context)?;
    println!("   Template: {}", greeting_template.template);
    println!("   Context: {:?}", context);
    println!("   Result: {}\n", result);

    // Example 2: Using PromptManager
    println!("2. Using PromptManager:");
    let mut manager = PromptManager::new();
    manager.register_template("greeting", greeting_template);

    let greeting_result = manager.render_template("greeting", &context)?;
    println!("   Managed template result: {}\n", greeting_result);

    // Example 3: Using predefined system prompt
    println!("3. Using Predefined System Prompt:");
    let system_template = presets::default_system_prompt();
    let sys_context = HashMap::from([
        (
            "context".to_string(),
            "AI assistant conversation".to_string(),
        ),
        (
            "history".to_string(),
            "User asked for help with coding".to_string(),
        ),
    ]);
    let system_result = system_template.render(&sys_context)?;
    println!("   System prompt: {}\n", system_result);

    // Example 4: Tool usage prompt
    println!("4. Tool Usage Prompt:");
    let tool_template = presets::tool_usage_prompt();
    let tool_context = HashMap::from([
        (
            "available_tools".to_string(),
            "Read, Write, Bash".to_string(),
        ),
        (
            "conversation_history".to_string(),
            "User wants to create a file".to_string(),
        ),
        (
            "user_input".to_string(),
            "Please create a hello.txt file with 'Hello, World!'".to_string(),
        ),
    ]);
    let tool_result = tool_template.render(&tool_context)?;
    println!(
        "   Tool prompt preview (first 200 chars): {}",
        &tool_result.chars().take(200).collect::<String>()
    );
    println!("   ...\n");

    // Example 5: Using PromptContext builder
    println!("5. Using PromptContext Builder:");
    let advanced_context = PromptContext::new()
        .add("persona", "Technical Support Specialist")
        .add("issue_type", "Bug Report")
        .add("urgency", "High")
        .add("customer_name", "John Doe");

    let advanced_template = PromptTemplate::new(
        "Customer: {{customer_name}}\nIssue Type: {{issue_type}}\nUrgency: {{urgency}}\n\nAs a {{persona}}, respond appropriately.".to_string(),
        "Advanced support template".to_string(),
    );

    let advanced_result = advanced_template.render(&advanced_context.variables)?;
    println!("   Advanced context result:\n{}", advanced_result);

    println!("\n=== Demo Complete ===");
    Ok(())
}
