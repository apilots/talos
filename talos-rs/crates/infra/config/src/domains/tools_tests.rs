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
        WebSearchProviderConfig::DuckDuckGoInstantAnswer
    );
    assert!(config.web_search.brave.api_key_env.is_some());
    assert!(config.web_search.tavily.api_key_env.is_some());
}

#[test]
fn web_search_provider_config_deserializes_from_snake_case() {
    let config: ToolsRuntimeConfig = serde_json::from_value(serde_json::json!({
        "web_search": {
            "provider": "duckduckgo",
            "duckduckgo": {
                "base_url": "https://duckduckgo.example/"
            }
        }
    }))
    .expect("web search config should deserialize");

    assert_eq!(
        config.web_search.provider,
        WebSearchProviderConfig::DuckDuckGoInstantAnswer
    );
    assert_eq!(
        config.web_search.duckduckgo.base_url.as_deref(),
        Some("https://duckduckgo.example/")
    );

    let config: ToolsRuntimeConfig = serde_json::from_value(serde_json::json!({
        "web_search": {
            "provider": "brave",
            "brave": {
                "api_key_env": "BRAVE_SEARCH_API_KEY",
                "base_url": "https://search.example/v1",
                "timeout_ms": 5000
            }
        }
    }))
    .expect("web search config should deserialize");

    assert_eq!(config.web_search.provider, WebSearchProviderConfig::Brave);
    assert_eq!(
        config.web_search.brave.api_key_env.as_deref(),
        Some("BRAVE_SEARCH_API_KEY")
    );
    assert_eq!(
        config.web_search.brave.base_url.as_deref(),
        Some("https://search.example/v1")
    );
    assert_eq!(config.web_search.brave.timeout_ms, 5000);
}
