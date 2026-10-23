use super::{
    ToolSandboxModeConfig, ToolSandboxNetworkConfig, ToolUserPermissionModeConfig,
    ToolsRuntimeConfig, WebSearchProviderConfig,
};

#[test]
fn bash_log_partial_config_preserves_defaults_and_rejects_invalid_quotas() {
    let mut config: super::BashOutputLogConfig =
        serde_json::from_value(serde_json::json!({"retention_days": 2})).unwrap();
    assert_eq!(config.max_file_bytes, 64 * 1024 * 1024);
    assert_eq!(config.max_workspace_bytes, 5 * 1024 * 1024 * 1024);
    assert_eq!(config.cleanup_interval_secs, 3600);
    assert!(config.validate().is_ok());
    config.max_workspace_bytes = config.max_file_bytes;
    assert!(config
        .validate()
        .unwrap_err()
        .is_code(talos_error::ErrorCode::InvalidRequest));
    config.max_workspace_bytes = 1024 * 1024 * 1024;
    config.retention_days = 0;
    assert!(config.validate().is_err());
}

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

#[test]
fn bash_config_rejects_old_name_and_zero_cleanup_interval() {
    assert!(
        serde_json::from_value::<ToolsRuntimeConfig>(serde_json::json!({"local_exec": {}}))
            .is_err()
    );
    let mut config = super::BashOutputLogConfig {
        cleanup_interval_secs: 0,
        ..super::BashOutputLogConfig::default()
    };
    assert!(config.validate().is_err());
    config.cleanup_interval_secs = u64::MAX;
    assert!(config.validate().is_err());
}

#[test]
fn maintained_yaml_templates_publish_identical_bash_log_defaults() {
    for name in [
        "conf/talos.yaml",
        "conf/talos-dev.yaml",
        "conf/talos-prod.yaml",
        "deploy/docker/talos-server/talos.yaml.example",
        "deploy/docker/talos-worker/talos.yaml.example",
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../..")
            .join(name);
        let value: serde_yaml::Value =
            serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let tools = &value["agent"]["kernel"]["tools"];
        assert!(tools.get("local_exec").is_none());
        let config: super::BashOutputLogConfig =
            serde_yaml::from_value(tools["bash"]["output_log"].clone()).unwrap();
        assert_eq!(config, super::BashOutputLogConfig::default());
    }
}
