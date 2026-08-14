use super::{
    ToolSandboxModeConfig, ToolSandboxNetworkConfig, ToolUserPermissionModeConfig,
    ToolsRuntimeConfig, WebSearchProviderConfig,
};

#[test]
fn collaboration_tools_are_enabled_by_default() {
    let config = ToolsRuntimeConfig::default();
    assert!(config.enable_collaboration_tools);
}

#[test]
fn sandbox_defaults_preserve_full_access_runtime_behavior() {
    let policy = ToolsRuntimeConfig::default().sandbox_policy;

    assert_eq!(
        policy.user_permission_mode,
        ToolUserPermissionModeConfig::FullAccess
    );
    assert_eq!(policy.default_mode, ToolSandboxModeConfig::UseDefault);
    assert_eq!(policy.network, ToolSandboxNetworkConfig::UseDefault);
    assert!(policy.platform.linux_helper_executable.is_none());
}

#[test]
fn sandbox_policy_deserializes_all_product_modes() {
    for (value, expected) in [
        (
            "ask_for_approval",
            ToolUserPermissionModeConfig::AskForApproval,
        ),
        ("approve_for_me", ToolUserPermissionModeConfig::ApproveForMe),
        ("full_access", ToolUserPermissionModeConfig::FullAccess),
    ] {
        let config: ToolsRuntimeConfig = serde_json::from_value(serde_json::json!({
            "sandbox_policy": {
                "user_permission_mode": value,
                "default_mode": "workspace_write",
                "network": "restricted",
                "platform": {
                    "linux_helper_executable": "/usr/libexec/talos-linux-sandbox-launcher"
                }
            }
        }))
        .expect("sandbox config should deserialize");

        assert_eq!(config.sandbox_policy.user_permission_mode, expected);
        assert_eq!(
            config.sandbox_policy.default_mode,
            ToolSandboxModeConfig::WorkspaceWrite
        );
        assert_eq!(
            config.sandbox_policy.network,
            ToolSandboxNetworkConfig::Restricted
        );
        assert_eq!(
            config
                .sandbox_policy
                .platform
                .linux_helper_executable
                .as_deref(),
            Some(std::path::Path::new(
                "/usr/libexec/talos-linux-sandbox-launcher"
            ))
        );
    }
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
    assert!(config.web_search.brave.api_key_env.is_some());
    assert!(config.web_search.tavily.api_key_env.is_some());
}

#[test]
fn web_search_provider_config_deserializes_from_snake_case() {
    let config: ToolsRuntimeConfig = serde_json::from_value(serde_json::json!({
        "web_search": {
            "provider": "firecrawl",
            "firecrawl": {
                "api_key_env": "FIRECRAWL_API_KEY",
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
        config.web_search.firecrawl.api_key_env.as_deref(),
        Some("FIRECRAWL_API_KEY")
    );
    assert_eq!(
        config.web_search.firecrawl.base_url.as_deref(),
        Some("https://firecrawl.example")
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
