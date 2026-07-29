use super::*;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_log_path(name: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    std::env::temp_dir()
        .join(format!("talos-logging-{}-{}", std::process::id(), unique))
        .join(name)
}

#[test]
fn startup_logging_config_uses_raw_logging_node_with_stable_file() {
    let log_path = temp_log_path("startup/talos.log");
    let config_path = log_path
        .parent()
        .expect("log parent should exist")
        .join("talos.yaml");
    fs::create_dir_all(config_path.parent().expect("config parent should exist"))
        .expect("config parent should be created");
    fs::write(
        &config_path,
        "logging:\n  level: debug\n  format: json\n  stdout: true\n",
    )
    .expect("config should be written");

    let (config, warning) = startup_logging_config(&StartupLoggingOptions {
        config_file: config_path,
        log_file: log_path.clone(),
        stdout: false,
    });

    assert_eq!(warning, None);
    assert_eq!(config.level, "debug");
    assert_eq!(config.format, "json");
    assert_eq!(config.file, Some(log_path.to_string_lossy().to_string()));
    assert!(!config.stdout);
}

#[test]
fn startup_logging_config_falls_back_when_yaml_is_malformed() {
    let log_path = temp_log_path("startup-fallback/talos.log");
    let config_path = log_path
        .parent()
        .expect("log parent should exist")
        .join("talos.yaml");
    fs::create_dir_all(config_path.parent().expect("config parent should exist"))
        .expect("config parent should be created");
    fs::write(&config_path, "logging: [").expect("config should be written");

    let (config, warning) = startup_logging_config(&StartupLoggingOptions {
        config_file: config_path,
        log_file: log_path.clone(),
        stdout: false,
    });

    assert!(warning
        .expect("malformed yaml should produce warning")
        .contains("failed to parse logging config yaml"));
    assert_eq!(config.level, "info");
    assert_eq!(config.file, Some(log_path.to_string_lossy().to_string()));
    assert!(!config.stdout);
}

#[test]
fn test_build_dispatch_rejects_empty_outputs() {
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "text".to_string(),
        file: None,
        stdout: false,
        ..Default::default()
    };

    let err = build_dispatch(&config).expect_err("empty logging outputs should fail");
    let message = err.to_string();
    assert!(message.contains("at least one output"));
}

#[test]
fn test_build_dispatch_writes_text_logs_to_file() {
    let log_path = temp_log_path("talos.log");
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "text".to_string(),
        file: Some(log_path.to_string_lossy().to_string()),
        stdout: false,
        ..Default::default()
    };

    let (dispatch, guard) = build_dispatch(&config).expect("dispatch should build");
    tracing::dispatcher::with_default(&dispatch, || {
        tracing::info!("file sink is active");
    });
    drop(dispatch);
    drop(guard);

    let content = fs::read_to_string(&log_path).expect("log file should exist");
    assert!(content.contains("file sink is active"));
}

#[test]
fn test_build_dispatch_writes_json_logs_to_file() {
    let log_path = temp_log_path("talos.json.log");
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "json".to_string(),
        file: Some(log_path.to_string_lossy().to_string()),
        stdout: false,
        ..Default::default()
    };

    let (dispatch, guard) = build_dispatch(&config).expect("dispatch should build");
    tracing::dispatcher::with_default(&dispatch, || {
        tracing::info!(target: "talos::test", "json sink is active");
    });
    drop(dispatch);
    drop(guard);

    let content = fs::read_to_string(&log_path).expect("log file should exist");
    assert!(content.starts_with('{'));
    assert!(content.contains("\"fields\":{\"message\":\"json sink is active\"}"));
}

#[test]
fn test_build_dispatch_creates_parent_directory_for_file_logs() {
    let log_path = temp_log_path("nested/logs/talos.log");
    let parent = log_path.parent().expect("parent dir should exist");
    let config = LoggingConfig {
        level: "info".to_string(),
        format: "text".to_string(),
        file: Some(log_path.to_string_lossy().to_string()),
        stdout: false,
        ..Default::default()
    };

    assert!(!parent.exists(), "test precondition failed");
    let (_dispatch, guard) = build_dispatch(&config).expect("dispatch should build");
    drop(guard);

    assert!(parent.exists(), "log parent directory should be created");
}
