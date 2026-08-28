use super::*;

#[test]
fn inherited_tool_environment_excludes_sensitive_values() {
    let environment = filter_inherited_tool_environment([
        ("PATH", "/usr/bin"),
        ("LANG", "en_US.UTF-8"),
        ("TALOS_LLM_ANTHROPIC_API_KEY", "secret-api-key"),
        ("TALOS_CLOUD_API_TOKEN", "secret-token"),
        ("CLIENT_SECRET", "secret-client-value"),
        ("TOKENIZERS_PARALLELISM", "false"),
    ]);

    assert_eq!(
        environment,
        BTreeMap::from([
            ("LANG".to_string(), "en_US.UTF-8".to_string()),
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("TOKENIZERS_PARALLELISM".to_string(), "false".to_string()),
        ])
    );
}
