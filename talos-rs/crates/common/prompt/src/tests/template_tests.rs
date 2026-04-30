use super::*;

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
