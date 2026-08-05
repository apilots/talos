use super::{ToolsRuntimeConfig, WebSearchProviderConfig};

#[test]
fn collaboration_tools_are_enabled_by_default() {
    let config = ToolsRuntimeConfig::default();
    assert!(config.enable_collaboration_tools);
}

#[test]
fn web_search_defaults_to_keyless_base_provider() {
    let config = ToolsRuntimeConfig::default();
    assert_eq!(
        config.web_search.provider,
        WebSearchProviderConfig::Firecrawl
    );
    assert_eq!(
        config.web_search.firecrawl.base_url.as_deref(),
        Some("https://api.firecrawl.dev")
    );
    assert_eq!(
        config.web_search.brave.api_key.as_deref(),
        Some("${TALOS_WEB_SEARCH_BRAVE_API_KEY}")
    );
    assert_eq!(
        config.web_search.tavily.api_key.as_deref(),
        Some("${TALOS_WEB_SEARCH_TAVILY_API_KEY}")
    );
}

#[test]
fn web_search_provider_config_deserializes_from_snake_case() {
    let config: ToolsRuntimeConfig = serde_json::from_value(serde_json::json!({
        "web_search": {
            "provider": "firecrawl",
            "firecrawl": {
                "api_key": "${FIRECRAWL_API_KEY}",
                "base_url": "https://firecrawl.example"
            }
        }
    }))
    .expect("web search config should deserialize");

    assert_eq!(
        config.web_search.provider,
        WebSearchProviderConfig::Firecrawl
    );
    assert_eq!(
        config.web_search.firecrawl.api_key.as_deref(),
        Some("${FIRECRAWL_API_KEY}")
    );
    assert_eq!(
        config.web_search.firecrawl.base_url.as_deref(),
        Some("https://firecrawl.example")
    );

    let config: ToolsRuntimeConfig = serde_json::from_value(serde_json::json!({
        "web_search": {
            "provider": "brave",
            "brave": {
                "api_key": "${BRAVE_SEARCH_API_KEY}",
                "base_url": "https://search.example/v1",
                "timeout_ms": 5000
            }
        }
    }))
    .expect("web search config should deserialize");

    assert_eq!(config.web_search.provider, WebSearchProviderConfig::Brave);
    assert_eq!(
        config.web_search.brave.api_key.as_deref(),
        Some("${BRAVE_SEARCH_API_KEY}")
    );
    assert_eq!(
        config.web_search.brave.base_url.as_deref(),
        Some("https://search.example/v1")
    );
    assert_eq!(config.web_search.brave.timeout_ms, 5000);

    let config: ToolsRuntimeConfig = serde_json::from_value(serde_json::json!({
        "web_search": {
            "provider": "tavily",
            "tavily": {
                "api_key": "test-key",
                "base_url": "https://tavily.example"
            }
        }
    }))
    .expect("web search config should deserialize");

    assert_eq!(config.web_search.provider, WebSearchProviderConfig::Tavily);
    assert_eq!(
        config.web_search.tavily.api_key.as_deref(),
        Some("test-key")
    );
    assert_eq!(
        config.web_search.tavily.base_url.as_deref(),
        Some("https://tavily.example")
    );
}
