use super::*;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn codec_noise_is_precise_and_explicitly_enableable() {
    for (name, level) in [("quiet", LogLevel::Warn), ("diagnostic", LogLevel::Info)] {
        let path = temp_log_path(name);
        let mut config = LoggingConfig {
            file: Some(path.to_string_lossy().into()),
            stdout: false,
            ..Default::default()
        };
        config.target_levels.insert(
            "loro_internal::oplog::change_store::block_encode".into(),
            level,
        );
        // Target overrides cannot silence a product handling-owner report.
        config
            .target_levels
            .insert("talos::diagnostic".into(), LogLevel::Off);
        let (dispatch, guard) = build_dispatch(&config, "UTC").unwrap();
        tracing::dispatcher::with_default(&dispatch, || {
            tracing::info!(target: "loro_internal::oplog::change_store::block_encode", "codec-info");
            tracing::warn!(target: "loro_internal::oplog::change_store::block_encode", "codec-warning");
            tracing::info!(target: "talos_sync::runner", "transport-started");
            report_error(&talos_error::TalosError::new(
                talos_error::ErrorCode::ActiveTurnRunning,
                "protected-turn",
            ));
        });
        drop(dispatch);
        drop(guard);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.contains("codec-info"), name == "diagnostic");
        for expected in [
            "codec-warning",
            "transport-started",
            "protected-turn",
            "backtrace",
        ] {
            assert!(text.contains(expected));
        }
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[test]
fn rotation_limits_and_maintained_templates_use_the_same_config() {
    let config = LoggingConfig {
        rotation: Some(LogRotation {
            max_files: 0,
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(
        build_dispatch(&config, "UTC").unwrap_err().error_code(),
        talos_error::ErrorCode::InvalidRequest
    );
    for filename in ["talos.yaml", "talos-dev.yaml", "talos-prod.yaml"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../conf")
            .join(filename);
        let value: serde_yaml::Value =
            serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        let actual: LoggingConfig = serde_yaml::from_value(value["logging"].clone()).unwrap();
        assert_eq!(
            serde_yaml::to_value(actual).unwrap(),
            serde_yaml::to_value(LoggingConfig::default()).unwrap()
        );
    }
    let disabled: LoggingConfig =
        serde_yaml::from_str("rotation: null\ntarget_levels: {}\n").unwrap();
    assert!(disabled.rotation.is_none());
    assert!(disabled.target_levels.is_empty());
}

#[test]
fn business_and_internal_reports_survive_verbosity_filter_and_flush() {
    let log_path = temp_log_path("failures.log");
    let config = LoggingConfig {
        level: "error,talos::diagnostic=off".to_string(),
        file: Some(log_path.to_string_lossy().to_string()),
        stdout: false,
        ..Default::default()
    };
    let errors = [
        talos_error::TalosError::from_source(
            talos_error::ErrorCode::PermissionDenied,
            "read session",
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied"),
        ),
        talos_error::TalosError::internal_with_source(
            "start service",
            std::io::Error::other("storage unavailable"),
        ),
    ];
    let (dispatch, guard) = build_dispatch(&config, "UTC").unwrap();
    tracing::dispatcher::with_default(&dispatch, || {
        for error in &errors {
            report_error(error);
        }
    });
    drop(dispatch);
    drop(guard);
    let content = fs::read_to_string(&log_path).unwrap();
    assert_eq!(content.matches("application failure").count(), 2);
    for expected in [
        "Business",
        "Internal",
        "access denied",
        "storage unavailable",
        "backtrace_status",
        "location",
        "read session",
        "start service",
    ] {
        assert!(
            content.contains(expected),
            "missing {expected} in diagnostic log"
        );
    }
}

#[test]
fn saturated_file_queue_preserves_every_write_and_flushes_on_shutdown() {
    use std::io::Write;
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;

    #[derive(Debug, PartialEq)]
    enum SinkEvent {
        Write(Vec<u8>),
        Flush,
    }

    struct ControlledWriter {
        entered: mpsc::Sender<()>,
        release: Option<mpsc::Receiver<()>>,
        events: Arc<Mutex<Vec<SinkEvent>>>,
    }

    impl Write for ControlledWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if let Some(release) = self.release.take() {
                self.entered.send(()).unwrap();
                release.recv_timeout(Duration::from_secs(10)).unwrap();
            }
            self.events
                .lock()
                .unwrap()
                .push(SinkEvent::Write(bytes.to_vec()));
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.events.lock().unwrap().push(SinkEvent::Flush);
            Ok(())
        }
    }

    let (entered, first_write) = mpsc::channel();
    let (release, blocked_writer) = mpsc::channel();
    let (write_completed, completion) = mpsc::channel::<()>();
    let events = Arc::new(Mutex::new(Vec::new()));
    let (mut writer, guard) =
        file_writer_builder()
            .buffered_lines_limit(1)
            .finish(ControlledWriter {
                entered,
                release: Some(blocked_writer),
                events: Arc::clone(&events),
            });
    writer.write_all(b"first").unwrap();
    first_write.recv_timeout(Duration::from_secs(10)).unwrap();
    writer.write_all(b"queued").unwrap();
    let unblock = std::thread::spawn(move || {
        // Release after a lossy write completes or the blocked lossless write's guard expires.
        // Correctness depends on retained bytes, not on elapsed time.
        assert!(completion.recv_timeout(Duration::from_secs(1)).is_err());
        release.send(()).unwrap();
    });
    writer.write_all(b"backpressured").unwrap();
    drop(write_completed);
    unblock.join().unwrap();
    drop(writer);
    drop(guard);
    let events = events.lock().unwrap();
    let writes: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            SinkEvent::Write(bytes) => Some(bytes.as_slice()),
            SinkEvent::Flush => None,
        })
        .collect();
    assert_eq!(
        (writes, events.last()),
        (
            vec![b"first".as_slice(), b"queued", b"backpressured"],
            Some(&SinkEvent::Flush)
        )
    );
}

pub(super) fn temp_log_path(name: &str) -> PathBuf {
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

    let (config, timezone, warning) = startup_logging_config(&StartupLoggingOptions {
        config_file: config_path,
        log_file: log_path.clone(),
        stdout: false,
    });

    assert_eq!(warning, None);
    assert_eq!(timezone, "UTC");
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

    let (config, timezone, warning) = startup_logging_config(&StartupLoggingOptions {
        config_file: config_path,
        log_file: log_path.clone(),
        stdout: false,
    });

    assert_eq!(timezone, "UTC");
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

    let err = build_dispatch(&config, "UTC").expect_err("empty logging outputs should fail");
    let message = err.to_string();
    assert!(message.contains("at least one output"));
}

#[test]
fn test_build_dispatch_writes_text_logs_to_file() {
    let log_path = temp_log_path("talos.log");
    let config = LoggingConfig {
        level: "trace,rmcp::transport::auth[grant]=trace".to_string(),
        format: "text".to_string(),
        file: Some(log_path.to_string_lossy().to_string()),
        stdout: false,
        ..Default::default()
    };

    let (dispatch, guard) =
        build_dispatch(&config, "Asia/Shanghai").expect("dispatch should build");
    tracing::dispatcher::with_default(&dispatch, || {
        tracing::info!("file sink is active");
        let span = tracing::trace_span!(target: "rmcp::transport::auth", "grant");
        let _entered = span.enter();
        tracing::debug!(target: "rmcp::transport::auth", code = "private-authorization-code", "callback");
        tracing::warn!(target: "rmcp::transport::auth", "private-provider-body");
    });
    drop(dispatch);
    drop(guard);

    let content = fs::read_to_string(&log_path).expect("daily log file should exist");
    assert!(content.contains("file sink is active"));
    let timestamp = content
        .split_whitespace()
        .next()
        .unwrap()
        .parse::<chrono::DateTime<chrono::FixedOffset>>()
        .unwrap();
    assert_eq!(timestamp.offset().local_minus_utc(), 8 * 3600);
    assert!(!content.contains("private-"));
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

    let (dispatch, guard) = build_dispatch(&config, "UTC").expect("dispatch should build");
    tracing::dispatcher::with_default(&dispatch, || {
        tracing::info!(target: "talos::test", "json sink is active");
    });
    drop(dispatch);
    drop(guard);

    let content = fs::read_to_string(&log_path).expect("daily log file should exist");
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
    let (_dispatch, guard) = build_dispatch(&config, "UTC").expect("dispatch should build");
    drop(guard);

    assert!(parent.exists(), "log parent directory should be created");
}

#[test]
fn startup_logging_uses_only_the_root_timezone() {
    let log_path = temp_log_path("timezone/talos.log");
    let config_path = log_path.parent().unwrap().join("talos.yaml");
    fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    for node in [
        "logging: { timezone: America/New_York }",
        "logging: { level: debug }",
        "",
    ] {
        fs::write(&config_path, format!("timezone: Asia/Shanghai\n{node}\n")).unwrap();
        let (_, timezone, warning) = startup_logging_config(&StartupLoggingOptions {
            config_file: config_path.clone(),
            log_file: log_path.clone(),
            stdout: false,
        });
        assert!(warning.is_none());
        assert_eq!(timezone, "Asia/Shanghai");
    }
    fs::remove_dir_all(log_path.parent().unwrap()).unwrap();
}

#[test]
fn invalid_timezone_and_zero_size_fail_explicitly() {
    assert_eq!(
        build_dispatch(&LoggingConfig::default(), "Mars/Olympus")
            .unwrap_err()
            .error_code(),
        talos_error::ErrorCode::InvalidRequest
    );
    for rotation in [
        LogRotation {
            max_bytes: 0,
            ..Default::default()
        },
        LogRotation {
            max_files: 1,
            ..Default::default()
        },
    ] {
        let config = LoggingConfig {
            rotation: Some(rotation),
            ..Default::default()
        };
        assert_eq!(
            build_dispatch(&config, "UTC").unwrap_err().error_code(),
            talos_error::ErrorCode::InvalidRequest
        );
    }
}
