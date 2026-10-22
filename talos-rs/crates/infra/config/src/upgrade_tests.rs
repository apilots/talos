use super::*;
fn write_template(root: &Path) -> PathBuf {
    let template_path = root.join("talos.yaml.template");
    let template =
        serde_yaml::to_string(&template_yaml()).expect("default config should serialize");
    std::fs::write(&template_path, template).expect("template should be written");
    template_path
}

fn write_existing(paths: &TalosPaths, value: &YamlValue) {
    std::fs::create_dir_all(&paths.conf_dir).expect("config directory should be created");
    let yaml = serde_yaml::to_string(value).expect("yaml should serialize");
    std::fs::write(&paths.config_file, yaml).expect("existing config should be written");
}

fn default_yaml() -> YamlValue {
    template_yaml()
}

#[test]
fn upgrade_removes_retired_config_without_changing_worker_identity() {
    for (transport, central) in [("outbound", false), ("direct", false), ("direct", true)] {
        let dir = tempfile::tempdir().unwrap();
        let paths = TalosPaths::from_home(dir.path().join(".talos"));
        let template_path = write_template(dir.path());
        let mut old = template_yaml();
        set_path_for_test(&mut old, &["gateway", "auth"], YamlValue::Bool(true));
        set_path_for_test(&mut old, &["mode"], YamlValue::String("cloud".into()));
        set_path_for_test(
            &mut old,
            &["desktop"],
            serde_yaml::from_str("connection_mode: local\nenrollment_pending: true\n").unwrap(),
        );
        let mut runner: YamlValue = serde_yaml::from_str("node_id: a03f484f-dfb3-4bf8-8796-35c6ce239d31\ncontrol_plane_endpoint: https://agent.example\ncredential_file: /private/node.credential\ntransport: outbound\ntrusted_ca_file: /private/ca.crt\n").unwrap();
        let credential = paths.conf_dir.join("node.credential");
        set_path_for_test(
            &mut runner,
            &["credential_file"],
            serde_yaml::to_value(&credential).unwrap(),
        );
        set_path_for_test(&mut old, &["agent_node", "runner"], runner.clone());
        if transport == "direct" {
            set_path_for_test(
                &mut runner,
                &["transport"],
                YamlValue::String("direct".into()),
            );
            set_path_for_test(&mut old, &["agent_node", "runner"], runner.clone());
            let tls: YamlValue = serde_yaml::from_str(
                "certificate_file: /private/worker.crt\nprivate_key_file: /private/worker.key\n",
            )
            .unwrap();
            set_path_for_test(&mut old, &["agent_node", "worker_rpc", "tls"], tls);
        }
        if central {
            let control: YamlValue = serde_yaml::from_str("bind_address: 127.0.0.1:18800\ntls_certificate_file: /private/server.crt\ntls_private_key_file: /private/server.key\ntrusted_node_ca_file: /private/ca.crt\n").unwrap();
            set_path_for_test(&mut old, &["agent_node", "control_plane"], control);
            set_path_for_test(
                &mut old,
                &["agent_node", "worker_rpc", "tls"],
                YamlValue::Null,
            );
        }
        let expected_nodes = get_path_for_test(&old, &["agent_node"]).unwrap().clone();
        // The old document differs from the new strict schema only in retired modes.
        let mut current = old.clone();
        remove_path_for_test(&mut current, &["mode"]);
        remove_path_for_test(&mut current, &["desktop"]);
        validate_config_value(&current, &RuntimeEnv::default()).unwrap();
        write_existing(&paths, &old);
        std::fs::write(&credential, "existing-registration-credential").unwrap();

        let report = upgrade_default_config_from_template(&paths, &template_path).unwrap();
        let mut removed = report.removed_paths.clone();
        removed.sort();
        assert_eq!(removed, vec!["desktop", "mode"]);
        assert_eq!(
            read_yaml_file(report.backup_path.as_ref().unwrap()).unwrap(),
            old
        );
        let normalized = read_yaml_file(&paths.config_file).unwrap();
        assert_eq!(
            get_path_for_test(&normalized, &["agent_node"]),
            Some(&expected_nodes)
        );
        assert_eq!(normalized, current);
        assert!(!paths.talos_home.join(".worker-state.json").exists());
        assert!(!paths.talos_home.join(".worker-state.lock").exists());
        assert_eq!(
            std::fs::read_to_string(credential).unwrap(),
            "existing-registration-credential"
        );
        assert!(
            !upgrade_default_config_from_template(&paths, &template_path)
                .unwrap()
                .changed
        );
    }
}

#[test]
fn managed_profile_upgrade_preserves_saved_runtime_preferences() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..");
    for profile in ["talos.yaml", "talos-prod.yaml", "talos-dev.yaml"] {
        let dir = tempfile::tempdir().unwrap();
        let paths = TalosPaths::from_home(dir.path().join("home"));
        let template_path = repo_root.join("conf").join(profile);
        let mut saved = read_yaml_file(&template_path).unwrap();
        for (path, value) in [
            (vec!["sync", "enabled"], "false"),
            (vec!["agent", "identity"], "PERSONA.md"),
            (
                vec!["agent", "message_queue", "max_messages_per_session"],
                "23",
            ),
            (vec!["agent", "timeline_replay_batch_size"], "128"),
            (vec!["agent", "command_idempotency_ttl_hours"], "72"),
            (
                vec!["agent", "agents"],
                "[{id: researcher, name: Researcher, enabled: true}]",
            ),
            (
                vec!["agent", "kernel", "compaction", "model"],
                "deepseek-v4-flash",
            ),
            (
                vec![
                    "agent",
                    "kernel",
                    "tools",
                    "local_exec",
                    "default_timeout_ms",
                ],
                "90000",
            ),
            (
                vec!["agent", "kernel", "tools", "enable_dynamic_tools"],
                "true",
            ),
            (
                vec!["agent", "kernel", "tools", "dynamic", "response_timeout_ms"],
                "45000",
            ),
            (
                vec!["agent", "kernel", "tools", "mcp_runtime", "enable_tools"],
                "false",
            ),
            (vec!["logging", "timezone"], "UTC"),
            (vec!["logging", "target_levels"], "{talos_sync::runner: info, loro_internal::oplog::change_store::block_encode: debug}"),
            (vec!["llm", "ollama", "api_key"], "fixture-provider-key"),
            (
                vec!["channels", "discord", "token"],
                "fixture-channel-token",
            ),
            (vec!["channels", "discord", "allow_from"], "[fixture-user]"),
        ] {
            set_path_for_test(&mut saved, &path, serde_yaml::from_str(value).unwrap());
        }
        // A newly introduced nullable setting triggers normalization of a saved home.
        remove_path_for_test(&mut saved, &["sync", "trusted_ca_file"]);
        let expected: Config = serde_yaml::from_value(saved.clone()).unwrap();
        expected.validate_runtime().unwrap();
        write_existing(&paths, &saved);

        let report = upgrade_default_config_from_template(&paths, &template_path).unwrap();
        assert!(report.changed, "{profile}");
        assert!(
            report
                .removed_paths
                .contains(&"logging.timezone".to_string()),
            "{profile}"
        );
        assert_eq!(
            read_yaml_file(report.backup_path.as_ref().unwrap()).unwrap(),
            saved
        );
        let actual: Config =
            serde_yaml::from_value(read_yaml_file(&paths.config_file).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap(),
            "{profile}"
        );
        assert!(
            !upgrade_default_config_from_template(&paths, &template_path)
                .unwrap()
                .changed
        );
    }
}

#[test]
fn upgrade_refuses_to_erase_worker_identity_when_tls_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    let template_path = write_template(dir.path());
    let mut old = template_yaml();
    set_path_for_test(&mut old, &["mode"], YamlValue::String("local".into()));
    let runner = serde_yaml::from_str("node_id: a03f484f-dfb3-4bf8-8796-35c6ce239d31\ncontrol_plane_endpoint: https://agent.example\ncredential_file: /private/node.credential\ntransport: direct\n").unwrap();
    set_path_for_test(&mut old, &["agent_node", "runner"], runner);
    write_existing(&paths, &old);
    let original = std::fs::read(&paths.config_file).unwrap();

    // A damaged listener must require repair, never silently reset enrollment.
    let error = upgrade_default_config_from_template(&paths, &template_path).unwrap_err();
    assert_eq!(error.error_code(), talos_error::ErrorCode::InvalidRequest);
    assert_eq!(std::fs::read(&paths.config_file).unwrap(), original);
}

fn template_yaml() -> YamlValue {
    let mut value = serde_yaml::to_value(Config::default()).expect("default config should encode");
    remove_path_for_test(&mut value, &["logging", "file"]);
    remove_path_for_test(&mut value, &["storage", "connection_string"]);
    value
}

fn remove_path_for_test(root: &mut YamlValue, path: &[&str]) {
    let mut cursor = root;
    for segment in &path[..path.len() - 1] {
        let key = YamlValue::String((*segment).to_string());
        let YamlValue::Mapping(map) = cursor else {
            return;
        };
        let Some(next) = map.get_mut(&key) else {
            return;
        };
        cursor = next;
    }
    let YamlValue::Mapping(map) = cursor else {
        return;
    };
    map.remove(YamlValue::String(path[path.len() - 1].to_string()));
}

fn set_path_for_test(root: &mut YamlValue, path: &[&str], value: YamlValue) {
    let mut cursor = root;
    for segment in &path[..path.len() - 1] {
        let key = YamlValue::String((*segment).to_string());
        let YamlValue::Mapping(map) = cursor else {
            panic!("path segment should be a mapping: {segment}");
        };
        cursor = map
            .get_mut(&key)
            .unwrap_or_else(|| panic!("path segment should exist: {segment}"));
    }
    let YamlValue::Mapping(map) = cursor else {
        panic!("path parent should be a mapping");
    };
    map.insert(YamlValue::String(path[path.len() - 1].to_string()), value);
}

fn get_path_for_test<'a>(root: &'a YamlValue, path: &[&str]) -> Option<&'a YamlValue> {
    let mut cursor = root;
    for segment in path {
        let YamlValue::Mapping(map) = cursor else {
            return None;
        };
        cursor = map.get(YamlValue::String((*segment).to_string()))?;
    }
    Some(cursor)
}

#[test]
fn upgrade_removes_legacy_keys_and_defaults_invalid_values() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    let template_path = write_template(dir.path());
    let mut old = default_yaml();
    set_path_for_test(
        &mut old,
        &["agent", "kernel", "memory"],
        YamlValue::Mapping(Mapping::new()),
    );
    set_path_for_test(
        &mut old,
        &["agent", "kernel", "tools", "web_search", "provider"],
        YamlValue::String("duckduckgo".to_string()),
    );
    set_path_for_test(
        &mut old,
        &["agent", "kernel", "tools", "web_search", "duckduckgo"],
        YamlValue::Mapping(Mapping::new()),
    );
    set_path_for_test(
        &mut old,
        &["llm", "model"],
        YamlValue::String("user-model".to_string()),
    );
    write_existing(&paths, &old);

    let report = upgrade_default_config_from_template(&paths, &template_path)
        .expect("config should upgrade");

    assert!(report.changed);
    assert!(report
        .backup_path
        .as_ref()
        .is_some_and(|path| path.is_file()));
    assert!(report
        .removed_paths
        .iter()
        .any(|path| path == "agent.kernel.memory"));
    assert!(report
        .removed_paths
        .iter()
        .any(|path| path == "agent.kernel.tools.web_search.duckduckgo"));
    assert!(report
        .defaulted_paths
        .iter()
        .any(|path| path == "agent.kernel.tools.web_search.provider"));

    let normalized = read_yaml_file(&paths.config_file).expect("normalized config should read");
    assert!(get_path_for_test(&normalized, &["agent", "kernel", "memory"]).is_none());
    assert_eq!(
        get_path_for_test(
            &normalized,
            &["agent", "kernel", "tools", "web_search", "provider"]
        ),
        Some(&YamlValue::String("firecrawl".to_string()))
    );
    assert_eq!(
        get_path_for_test(&normalized, &["llm", "model"]),
        Some(&YamlValue::String("user-model".to_string()))
    );
    validate_config_value(&normalized, &RuntimeEnv::default())
        .expect("normalized config should strict-load");
}

#[test]
fn upgrade_adds_missing_current_defaults() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    let template_path = write_template(dir.path());
    let mut old = default_yaml();
    let YamlValue::Mapping(map) = &mut old else {
        panic!("default config should be a mapping");
    };
    map.remove(YamlValue::String("memory".to_string()));
    map.remove(YamlValue::String("automation".to_string()));
    map.remove(YamlValue::String("workspaces".to_string()));
    write_existing(&paths, &old);

    let report = upgrade_default_config_from_template(&paths, &template_path)
        .expect("config should upgrade");

    assert!(report.changed);
    assert!(report.added_paths.iter().any(|path| path == "memory"));
    assert!(report.added_paths.iter().any(|path| path == "automation"));
    assert!(report.added_paths.iter().any(|path| path == "workspaces"));
    let normalized = read_yaml_file(&paths.config_file).expect("normalized config should read");
    assert!(get_path_for_test(&normalized, &["memory"]).is_some());
    assert!(get_path_for_test(&normalized, &["automation"]).is_some());
    assert!(get_path_for_test(&normalized, &["workspaces"]).is_some());
    validate_config_value(&normalized, &RuntimeEnv::default())
        .expect("normalized config should strict-load");
}

#[test]
fn upgrade_preserves_configured_agent_node_listener() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    let template_path = dir.path().join("talos.yaml.template");
    let mut template = template_yaml();
    set_path_for_test(&mut template, &["gateway", "auth"], YamlValue::Bool(true));
    std::fs::write(
        &template_path,
        serde_yaml::to_string(&template).expect("template should encode"),
    )
    .expect("template should be written");
    let mut old = template;
    set_path_for_test(
        &mut old,
        &["agent_node", "control_plane"],
        serde_yaml::from_str(
            r#"
bind_address: 127.0.0.1:18800
tls_certificate_file: /tmp/control.crt
tls_private_key_file: /tmp/control.key
trusted_node_ca_file: /tmp/node-ca.crt
"#,
        )
        .expect("Agent node listener should encode"),
    );
    write_existing(&paths, &old);

    let report = upgrade_default_config_from_template(&paths, &template_path)
        .expect("config should upgrade");

    assert!(!report
        .removed_paths
        .iter()
        .any(|path| path.starts_with("agent_node")));
    let normalized = read_yaml_file(&paths.config_file).expect("normalized config should read");
    assert_eq!(
        get_path_for_test(
            &normalized,
            &["agent_node", "control_plane", "bind_address"]
        ),
        Some(&YamlValue::String("127.0.0.1:18800".to_string()))
    );
    validate_config_value(&normalized, &RuntimeEnv::default())
        .expect("normalized config should strict-load");
}

#[test]
fn refresh_template_copies_source_when_target_is_missing() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    let source_path = write_template(dir.path());

    let report = refresh_default_config_template_from_source(&paths, &source_path)
        .expect("template should refresh");

    assert!(report.changed);
    assert_eq!(report.source_path, source_path);
    assert_eq!(report.target_path, paths.config_template_file);
    assert_eq!(
        std::fs::read_to_string(&report.target_path).expect("target template should read"),
        std::fs::read_to_string(&report.source_path).expect("source template should read")
    );
}

#[test]
fn refresh_template_overwrites_stale_target() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    let source_path = write_template(dir.path());
    std::fs::create_dir_all(&paths.conf_dir).expect("config directory should be created");
    std::fs::write(&paths.config_template_file, "stale: true\n")
        .expect("stale template should be written");

    let report = refresh_default_config_template_from_source(&paths, &source_path)
        .expect("template should refresh");

    assert!(report.changed);
    assert_eq!(
        std::fs::read_to_string(&paths.config_template_file).expect("target template should read"),
        std::fs::read_to_string(&source_path).expect("source template should read")
    );
}

#[test]
fn refresh_template_reports_unchanged_target() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    let source_path = write_template(dir.path());
    refresh_default_config_template_from_source(&paths, &source_path)
        .expect("template should refresh first time");

    let report = refresh_default_config_template_from_source(&paths, &source_path)
        .expect("template should refresh second time");

    assert!(!report.changed);
}

#[test]
fn refresh_template_fails_when_source_is_missing() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    let source_path = dir.path().join("missing.yaml");

    let error = refresh_default_config_template_from_source(&paths, &source_path)
        .expect_err("missing source should fail");

    assert!(error
        .to_string()
        .contains("missing default config template source"));
}

#[test]
fn upgrade_preserves_dynamic_storage_backends_when_valid() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    let template_path = write_template(dir.path());
    let mut old = default_yaml();
    let s3 = serde_yaml::from_str::<YamlValue>(
        r#"
type: s3
root: /talos-files
bucket: talos-files
region: us-east-1
"#,
    )
    .expect("s3 backend should parse");
    set_path_for_test(&mut old, &["storage", "files", "backends", "s3"], s3);
    set_path_for_test(
        &mut old,
        &["storage", "files", "default"],
        YamlValue::String("s3".to_string()),
    );
    write_existing(&paths, &old);

    let report = upgrade_default_config_from_template(&paths, &template_path)
        .expect("config should upgrade");

    let normalized = read_yaml_file(&paths.config_file).expect("normalized config should read");
    assert!(get_path_for_test(&normalized, &["storage", "files", "backends", "s3"]).is_some());
    assert_eq!(
        get_path_for_test(&normalized, &["storage", "files", "default"]),
        Some(&YamlValue::String("s3".to_string()))
    );
    assert!(!report
        .removed_paths
        .iter()
        .any(|path| path == "storage.files.backends.s3"));
    validate_config_value(&normalized, &RuntimeEnv::default())
        .expect("normalized config should strict-load");
}

#[test]
fn upgrade_preserves_custom_default_storage_backend_without_template_local_backend() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    let template_path = write_template(dir.path());
    let mut old = default_yaml();
    let backends = serde_yaml::from_str::<YamlValue>(
        r#"
minio:
  type: s3
  root: /talos-e2e
  bucket: talos-files
  region: us-east-1
  endpoint: http://127.0.0.1:9000
  access_key_id: talos-files-e2e
  secret_access_key: talos-files-e2e-secret
"#,
    )
    .expect("s3 backends should parse");
    set_path_for_test(&mut old, &["storage", "files", "backends"], backends);
    set_path_for_test(
        &mut old,
        &["storage", "files", "default"],
        YamlValue::String("minio".to_string()),
    );
    write_existing(&paths, &old);

    let report = upgrade_default_config_from_template(&paths, &template_path)
        .expect("config should upgrade");

    let normalized = read_yaml_file(&paths.config_file).expect("normalized config should read");
    assert!(get_path_for_test(&normalized, &["storage", "files", "backends", "minio"]).is_some());
    assert_eq!(
        get_path_for_test(&normalized, &["storage", "files", "default"]),
        Some(&YamlValue::String("minio".to_string()))
    );
    assert!(!report
        .removed_paths
        .iter()
        .any(|path| path == "storage.files.backends.minio"));
    assert!(!report
        .defaulted_paths
        .iter()
        .any(|path| path == "storage.files.default"));
    validate_config_value(&normalized, &RuntimeEnv::default())
        .expect("normalized config should strict-load");
}

#[test]
fn upgrade_defaults_runtime_invalid_values() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    let template_path = write_template(dir.path());
    let mut old = default_yaml();
    set_path_for_test(
        &mut old,
        &["storage", "files", "default"],
        YamlValue::String("missing".to_string()),
    );
    write_existing(&paths, &old);

    let report = upgrade_default_config_from_template(&paths, &template_path)
        .expect("config should upgrade");

    assert!(report.changed);
    assert!(report
        .defaulted_paths
        .iter()
        .any(|path| path == "storage.files.default"));
    let normalized = read_yaml_file(&paths.config_file).expect("normalized config should read");
    assert_eq!(
        get_path_for_test(&normalized, &["storage", "files", "default"]),
        Some(&YamlValue::String("local".to_string()))
    );
    validate_config_value(&normalized, &RuntimeEnv::default())
        .expect("normalized config should strict-load");
}

#[test]
fn malformed_yaml_does_not_overwrite_existing_config() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    let template_path = write_template(dir.path());
    std::fs::create_dir_all(&paths.conf_dir).expect("config directory should be created");
    std::fs::write(&paths.config_file, "agent: [").expect("malformed config should be written");

    let error = upgrade_default_config_from_template(&paths, &template_path)
        .expect_err("malformed config should fail");

    assert!(error.to_string().contains("parse config yaml"));
    assert_eq!(
        std::fs::read_to_string(&paths.config_file).expect("config should remain readable"),
        "agent: ["
    );
    let backups = std::fs::read_dir(&paths.conf_dir)
        .expect("config dir should list")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".bak."))
        .count();
    assert_eq!(backups, 0);
}
