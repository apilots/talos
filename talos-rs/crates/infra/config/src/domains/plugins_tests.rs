use super::*;

#[test]
fn explicit_sources_preserve_skills_policy_and_only_add_the_new_mcp_source() {
    let custom = PluginSourceConfig {
        reference: "pinned-skills".into(),
        ..default_sources()[0].clone()
    };
    let private = PluginSourceConfig {
        source_type: PluginSourceType::User,
        display_name: "Private".into(),
        kind: PluginSourceKind::Git,
        url: "https://example.test/plugins".into(),
        reference: "stable".into(),
        sparse_paths: vec!["skills".into()],
    };
    for sources in [vec![], vec![custom], vec![private]] {
        let input = serde_json::json!({"sources": sources, "roots": []});
        let config: PluginsRuntimeConfig = serde_json::from_value(input).unwrap();
        let mut expected = sources;
        expected.push(official_mcp_source());
        assert_eq!(config.sources, expected);
        let reopened: PluginsRuntimeConfig =
            serde_json::from_value(serde_json::to_value(&config).unwrap()).unwrap();
        assert_eq!(reopened, config);
    }
    let omitted: PluginsRuntimeConfig = serde_json::from_value(serde_json::json!({})).unwrap();
    assert_eq!(omitted.sources, default_sources());
}
