use super::*;
use std::fs;

#[test]
fn startup_logging_keeps_early_settings_and_the_stable_default_file() {
    let home = tempfile::tempdir().unwrap();
    let paths = TalosPaths::from_home(home.path().to_path_buf());
    fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
    fs::write(
        &paths.config_file,
        "logging:\n  level: debug\n  format: json\n  stdout: true\n",
    )
    .unwrap();
    let (config, timezone, warning) = load_startup_logging(&paths).unwrap();
    assert!(warning.is_none());
    assert_eq!(timezone, default_timezone());
    assert_eq!(config.level, "debug");
    assert_eq!(config.format, "json");
    assert_eq!(config.file.as_deref(), paths.log_file.to_str());
    assert!(config.stdout);
}

#[test]
fn malformed_yaml_keeps_defaults_and_the_deferred_warning() {
    let home = tempfile::tempdir().unwrap();
    let paths = TalosPaths::from_home(home.path().to_path_buf());
    fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
    fs::write(&paths.config_file, "logging: [").unwrap();
    let (config, timezone, warning) = load_startup_logging(&paths).unwrap();
    assert_eq!(timezone, default_timezone());
    assert!(warning
        .unwrap()
        .contains("failed to parse logging config yaml"));
    assert_eq!(config.level, "info");
    assert_eq!(config.file.as_deref(), paths.log_file.to_str());
}

#[test]
fn startup_uses_only_the_root_timezone() {
    let home = tempfile::tempdir().unwrap();
    let paths = TalosPaths::from_home(home.path().to_path_buf());
    fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
    for node in [
        "logging: { timezone: America/New_York }",
        "logging: { level: debug }",
        "",
    ] {
        fs::write(
            &paths.config_file,
            format!("timezone: Asia/Shanghai\n{node}\n"),
        )
        .unwrap();
        let (_, timezone, warning) = load_startup_logging(&paths).unwrap();
        assert!(warning.is_none());
        assert_eq!(timezone, "Asia/Shanghai");
    }
}

#[test]
fn startup_and_full_loader_share_dotenv_and_process_placeholder_resolution() {
    let home = tempfile::tempdir().unwrap();
    let paths = TalosPaths::from_home(home.path().to_path_buf());
    fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
    fs::write(
        &paths.env_file,
        "TALOS_FIX55_STARTUP_TIMEZONE=Asia/Shanghai\nTALOS_FIX55_STARTUP_LEVEL=debug\n",
    )
    .unwrap();
    let template =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/talos-dev.yaml");
    let mut value: serde_json::Value =
        serde_yaml::from_str(&fs::read_to_string(template).unwrap()).unwrap();
    value["timezone"] = "${TALOS_FIX55_STARTUP_TIMEZONE}".into();
    value["logging"]["level"] = "${TALOS_FIX55_STARTUP_LEVEL}".into();
    fs::write(&paths.config_file, serde_yaml::to_string(&value).unwrap()).unwrap();
    let (logging, timezone, warning) = load_startup_logging(&paths).unwrap();
    assert!(warning.is_none());
    let full = loader::load_config_at(paths.config_file.clone(), paths.clone()).unwrap();
    assert_eq!(timezone, full.timezone);
    assert_eq!(timezone, "Asia/Shanghai");
    assert_eq!(logging.level, full.logging.level);
    assert_eq!(logging.level, "debug");
    let process_env = RuntimeEnv::load_from_iter(
        &paths.env_file,
        [("TALOS_FIX55_STARTUP_TIMEZONE", "America/New_York")],
    )
    .unwrap();
    let (_, timezone, warning) = startup_logging_config(&paths, &process_env);
    assert!(warning.is_none());
    let config: crate::Config = serde_json::from_value(value).unwrap();
    let resolved = loader::resolve_env_placeholders(config, &process_env).unwrap();
    assert_eq!(timezone, resolved.timezone);
    assert_eq!(timezone, "America/New_York");
}
