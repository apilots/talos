use super::*;

fn manifest(directory: &Path, browser: serde_json::Value) {
    std::fs::write(
        directory.join("talos-mcp.json"),
        serde_json::to_vec(&serde_json::json!({
            "name": "example", "default_install": true, "browser": browser,
            "transport": {"type": "stdio", "command": "example", "args": []}
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn browser_metadata_is_validated_without_leaking_into_connection() {
    let temporary = tempfile::tempdir().unwrap();
    manifest(
        temporary.path(),
        serde_json::json!({
            "api_version": 1, "docker_requirements": "requirements.lock"
        }),
    );
    assert!(requests_default_install(temporary.path()).unwrap());
    assert_eq!(
        browser_configuration(temporary.path()).unwrap(),
        Some(BrowserPluginConfig {
            api_version: 1,
            docker_requirements: "requirements.lock".into()
        })
    );
    let server = server_from_installed_mcp_manifest(temporary.path()).unwrap();
    assert_eq!(server.name, "example");
    assert!(!serde_json::to_value(server)
        .unwrap()
        .as_object()
        .unwrap()
        .contains_key("browser"));
}

#[test]
fn unsupported_browser_api_and_escaping_dependency_paths_are_rejected() {
    let temporary = tempfile::tempdir().unwrap();
    for (api, path) in [
        (2, "requirements.lock"),
        (1, "../requirements.lock"),
        (1, "/tmp/lock"),
        (1, ""),
    ] {
        manifest(
            temporary.path(),
            serde_json::json!({"api_version":api,"docker_requirements":path}),
        );
        assert!(server_from_installed_mcp_manifest(temporary.path()).is_err());
    }
}

#[test]
fn original_third_party_manifest_needs_no_browser_or_install_metadata() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::write(
        temporary.path().join("talos-mcp.json"),
        r#"{
        "name":"remote", "transport":{"type":"streamable_http","url":"https://example.com/mcp"}
    }"#,
    )
    .unwrap();
    assert!(!requests_default_install(temporary.path()).unwrap());
    assert_eq!(browser_configuration(temporary.path()).unwrap(), None);
    assert_eq!(
        server_from_installed_mcp_manifest(temporary.path())
            .unwrap()
            .name,
        "remote"
    );
}
