use super::*;
use talos_error::TalosError;
use talos_llm_provider::{
    CacheControl, CacheControlKind, ChatMessage, ChatRequest, LlmProvider, NativeThinkingConfig,
    NativeThinkingEffort, NativeThinkingSupport, PromptCacheSupport, ToolStream, ToolStreamEvent,
    Usage,
};
use tokio::sync::mpsc;

#[test]
fn test_bailian_config_builder() {
    let config = BailianConfig::new("test-api-key")
        .with_base_url("https://example.com/compatible-mode/v1")
        .with_timeout(120);

    assert_eq!(config.api_key, "test-api-key");
    assert_eq!(config.base_url, "https://example.com/compatible-mode/v1");
    assert_eq!(config.timeout_secs, 120);
}

#[test]
fn test_native_thinking_support_is_qwen_gated() {
    let provider = BailianProvider::new(BailianConfig::new("test-key"));
    assert_eq!(
        provider.prompt_cache_support(),
        PromptCacheSupport::RequestLevel
    );
    assert_eq!(
        provider.native_thinking_support("qwen-plus"),
        NativeThinkingSupport::Supported
    );
    assert_eq!(
        provider.native_thinking_support("qwen-plus-latest"),
        NativeThinkingSupport::Supported
    );
    assert_eq!(
        provider.native_thinking_support("qwen-max"),
        NativeThinkingSupport::Unsupported
    );
}

#[test]
fn test_build_request_maps_native_thinking_to_bailian_fields() {
    let request = ChatRequest::builder()
        .model("qwen-plus")
        .messages(vec![ChatMessage::user("hello")])
        .native_thinking(NativeThinkingConfig {
            effort: NativeThinkingEffort::Medium,
            budget_tokens: Some(1024),
        })
        .build();

    let built = BailianProvider::build_request(request, true);
    let json = serde_json::to_value(&built).expect("request should serialize");
    assert_eq!(json["enable_thinking"], true);
    assert_eq!(json["thinking_budget"], 1024);
    assert_eq!(json["stream"], true);
}

#[test]
fn test_extract_reasoning_delta_reads_reasoning_content() {
    let delta = serde_json::json!({
        "reasoning_content": [
            {"text": "first "},
            {"content": "second"}
        ]
    });
    assert_eq!(
        BailianProvider::extract_reasoning_delta(&delta).as_deref(),
        Some("first second")
    );
}

#[tokio::test]
async fn test_stream_done_event_shape_is_preserved() {
    let (tx, rx) = mpsc::channel(1);
    tx.send(Ok(ToolStreamEvent::Done {
        finish_reason: Some("stop".to_string()),
        usage: Some(Usage {
            prompt_tokens: 1,
            completion_tokens: 2,
            total_tokens: 3,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
    }))
    .await
    .expect("send should succeed");

    let mut stream = ToolStream::new(rx);
    let event = stream
        .next()
        .await
        .expect("event should exist")
        .expect("event should be ok");
    assert!(matches!(
        event,
        ToolStreamEvent::Done {
            finish_reason: Some(_),
            usage: Some(_)
        }
    ));
}

#[test]
fn test_bailian_request_serializes_cache_control_for_request_level_provider() {
    let provider = BailianProvider::new(BailianConfig::new("test-key"));
    assert_eq!(
        provider.prompt_cache_support(),
        PromptCacheSupport::RequestLevel
    );

    let mut message = ChatMessage::user("hello");
    message.cache_control = Some(CacheControl {
        kind: CacheControlKind::Ephemeral,
    });
    let request = BailianProvider::build_request(
        ChatRequest::builder()
            .model("qwen-plus")
            .messages(vec![message])
            .build(),
        false,
    );

    let json = serde_json::to_string(&request).expect("request should serialize");
    assert!(json.contains("cache_control"));
}

#[test]
fn test_bailian_request_serializes_cache_control_as_content_block() {
    let mut message = ChatMessage::system("stable context");
    message.cache_control = Some(CacheControl {
        kind: CacheControlKind::Ephemeral,
    });
    let request = BailianProvider::build_request(
        ChatRequest::builder()
            .model("qwen-plus")
            .messages(vec![message])
            .build(),
        false,
    );

    let json = serde_json::to_value(&request).expect("request should serialize");
    assert_eq!(json["messages"][0]["content"][0]["text"], "stable context");
    assert_eq!(
        json["messages"][0]["content"][0]["cache_control"]["type"],
        "ephemeral"
    );
}

#[test]
fn test_usage_from_bailian_maps_cache_fields() {
    let usage = BailianProvider::usage_from_bailian(BailianUsage {
        prompt_tokens: 128,
        completion_tokens: 32,
        total_tokens: 160,
        prompt_tokens_details: Some(BailianPromptTokensDetails {
            cached_tokens: Some(96),
        }),
        cache_creation: Some(BailianCacheCreation {
            cache_creation_input_tokens: Some(48),
            ephemeral_5m_input_tokens: Some(48),
        }),
    })
    .expect("usage mapping should succeed");

    assert_eq!(usage.prompt_tokens, 128);
    assert_eq!(usage.completion_tokens, 32);
    assert_eq!(usage.total_tokens, 160);
    assert_eq!(usage.cache_read_input_tokens, Some(96));
    assert_eq!(usage.cache_creation_input_tokens, Some(48));
}

#[test]
fn test_usage_from_bailian_maps_ephemeral_cache_creation_fallback() {
    let usage = BailianProvider::usage_from_bailian(BailianUsage {
        prompt_tokens: 256,
        completion_tokens: 64,
        total_tokens: 320,
        prompt_tokens_details: Some(BailianPromptTokensDetails {
            cached_tokens: Some(128),
        }),
        cache_creation: Some(BailianCacheCreation {
            cache_creation_input_tokens: None,
            ephemeral_5m_input_tokens: Some(72),
        }),
    })
    .expect("usage mapping should succeed");

    assert_eq!(usage.prompt_tokens, 256);
    assert_eq!(usage.completion_tokens, 64);
    assert_eq!(usage.total_tokens, 320);
    assert_eq!(usage.cache_read_input_tokens, Some(128));
    assert_eq!(usage.cache_creation_input_tokens, Some(72));
}

#[test]
fn test_usage_from_bailian_rejects_conflicting_cache_creation_fields() {
    let err = BailianProvider::usage_from_bailian(BailianUsage {
        prompt_tokens: 256,
        completion_tokens: 64,
        total_tokens: 320,
        prompt_tokens_details: Some(BailianPromptTokensDetails {
            cached_tokens: Some(128),
        }),
        cache_creation: Some(BailianCacheCreation {
            cache_creation_input_tokens: Some(72),
            ephemeral_5m_input_tokens: Some(96),
        }),
    })
    .expect_err("usage mapping should reject conflicting cache creation fields");

    assert!(matches!(err, TalosError::ValidationError(_)));
}
