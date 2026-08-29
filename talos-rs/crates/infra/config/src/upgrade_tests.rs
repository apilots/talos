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
    validate_config_value(&normalized).expect("normalized config should strict-load");
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
    validate_config_value(&normalized).expect("normalized config should strict-load");
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
    validate_config_value(&normalized).expect("normalized config should strict-load");
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
    validate_config_value(&normalized).expect("normalized config should strict-load");
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
    validate_config_value(&normalized).expect("normalized config should strict-load");
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
