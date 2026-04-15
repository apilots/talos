use super::*;
use talos_llm_provider::{
    CacheControl, CacheControlKind, ChatMessage, ChatRequest, LlmProvider, NativeThinkingConfig,
    NativeThinkingEffort, NativeThinkingSupport, PromptCacheSupport, ToolDefinition,
};

#[test]
fn test_anthropic_config_builder() {
    let config = AnthropicConfig::new("test-api-key")
        .with_base_url("https://custom.anthropic.com/v1")
        .with_timeout(120);

    assert_eq!(config.api_key, "test-api-key");
    assert_eq!(config.base_url, "https://custom.anthropic.com/v1");
    assert_eq!(config.timeout_secs, 120);
}

#[test]
fn test_convert_messages() {
    let messages = vec![
        ChatMessage::system("You are a helpful assistant"),
        ChatMessage::user("Hello"),
        ChatMessage::assistant("Hi there!"),
    ];

    let (system, anthropic_msgs) = AnthropicProvider::convert_messages(messages);
    let first = serde_json::to_value(&anthropic_msgs[0]).expect("message should serialize");
    let second = serde_json::to_value(&anthropic_msgs[1]).expect("message should serialize");

    assert_eq!(system.as_ref().map(|blocks| blocks.len()), Some(1));
    assert_eq!(anthropic_msgs.len(), 2);
    assert_eq!(first["role"], "user");
    assert!(first["content"].is_array());
    assert_eq!(second["role"], "assistant");
    assert!(second["content"].is_array());
}

#[test]
fn test_convert_messages_with_tool_result() {
    let messages = vec![ChatMessage::tool("call_1", "read", "done")];
    let (_system, anthropic_msgs) = AnthropicProvider::convert_messages(messages);
    let first = serde_json::to_value(&anthropic_msgs[0]).expect("message should serialize");
    assert_eq!(anthropic_msgs.len(), 1);
    assert_eq!(first["role"], "user");
    assert!(first["content"].is_array());
}

#[test]
fn test_provider_name() {
    let config = AnthropicConfig::new("test-key");
    let provider = AnthropicProvider::new(config);
    assert_eq!(provider.name(), "anthropic");
    assert_eq!(provider.default_model(), "claude-3-5-sonnet-20241022");
    assert_eq!(
        provider.prompt_cache_support(),
        PromptCacheSupport::RequestLevel
    );
}

#[test]
fn test_native_thinking_support_is_model_gated() {
    let provider = AnthropicProvider::new(AnthropicConfig::new("test-key"));

    assert_eq!(
        provider.native_thinking_support("claude-3-5-sonnet-20241022"),
        NativeThinkingSupport::Unsupported
    );
    assert_eq!(
        provider.native_thinking_support("claude-3-7-sonnet-20250219"),
        NativeThinkingSupport::Supported
    );
}

#[test]
fn test_build_headers() {
    let config = AnthropicConfig::new("test-api-key");
    let provider = AnthropicProvider::new(config);
    let headers = provider.build_headers();

    assert!(headers.contains_key("x-api-key"));
    assert!(headers.contains_key("anthropic-version"));
    assert!(headers.contains_key("accept"));
    assert!(headers.contains_key("content-type"));
}

#[test]
fn test_build_headers_for_aliyun_coding_plan() {
    let config = AnthropicConfig::new("test-api-key")
        .with_base_url("https://coding.dashscope.aliyuncs.com/apps/anthropic/v1");
    let provider = AnthropicProvider::new(config);
    let headers = provider.build_headers();

    assert!(!headers.contains_key("x-api-key"));
    assert_eq!(
        headers.get("accept").and_then(|v| v.to_str().ok()),
        Some("*/*")
    );
    assert!(headers.contains_key("authorization"));
    assert!(headers.contains_key("anthropic-beta"));
    assert_eq!(
        headers.get("x-app").and_then(|v| v.to_str().ok()),
        Some("cli")
    );
    assert_eq!(
        headers.get("user-agent").and_then(|v| v.to_str().ok()),
        Some("claude-cli/0.1.0 (external, cli)")
    );
    assert_eq!(
        headers.get("anthropic-beta").and_then(|v| v.to_str().ok()),
        Some("claude-code-20250219")
    );
}

#[test]
fn test_convert_tools() {
    let tools = vec![ToolDefinition::new(
        "search",
        "Search docs",
        Some(serde_json::json!({"type": "object"})),
    )];
    let converted = AnthropicProvider::convert_tools(Some(tools)).unwrap();
    let json = serde_json::to_value(&converted[0]).expect("tool should serialize");
    assert_eq!(converted.len(), 1);
    assert_eq!(json["name"], "search");
}

#[test]
fn test_build_request_maps_native_thinking_budget() {
    let request = ChatRequest::builder()
        .model("claude-3-5-sonnet-20241022")
        .messages(vec![ChatMessage::user("hello")])
        .native_thinking(NativeThinkingConfig {
            effort: NativeThinkingEffort::Medium,
            budget_tokens: Some(4096),
        })
        .build();

    let built = AnthropicProvider::build_request(request, None);
    let json = serde_json::to_value(&built).expect("request should serialize");

    assert_eq!(json["thinking"]["budget_tokens"], 4096);
}

#[test]
fn test_convert_messages_preserves_multiple_system_blocks_and_cache_control() {
    let mut first = ChatMessage::system("System text");
    first.cache_control = Some(CacheControl {
        kind: CacheControlKind::Ephemeral,
    });
    let messages = vec![first, ChatMessage::system("Workspace context")];

    let (system_blocks, anthropic_msgs) = AnthropicProvider::convert_messages(messages);
    let serialized = serde_json::to_value(&system_blocks.expect("system blocks should exist"))
        .expect("system blocks should serialize");

    assert!(anthropic_msgs.is_empty());
    assert_eq!(serialized.as_array().map(|items| items.len()), Some(2));
    assert_eq!(serialized[0]["cache_control"]["type"], "ephemeral");
    assert_eq!(serialized[1]["text"], "Workspace context");
}

#[test]
fn test_text_block_serialization_skips_null_cache_control() {
    let block = AnthropicProvider::text_block("hello".to_string(), None);
    let json = serde_json::to_string(&block).expect("text block should serialize");
    assert!(!json.contains("cache_control"));
    assert!(!json.contains("null"));
}
