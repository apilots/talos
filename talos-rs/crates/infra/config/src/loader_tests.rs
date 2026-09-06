use super::*;

#[test]
fn config_file_failures_preserve_the_filesystem_cause_and_public_explanation() {
    let directory = tempfile::tempdir().unwrap();
    let paths = TalosPaths::from_home(directory.path().join("home"));
    paths.ensure_base_dirs().unwrap();
    std::fs::create_dir(&paths.config_file).unwrap();
    let resource = write_resource_template(directory.path(), "gateway: {}\n");
    let template = default_config_template_path(resource);
    let config = Config::default();
    let yaml = serde_yaml::to_string(&config).unwrap();
    let failures = [
        (
            copy_default_config_template_from_candidates(&paths, vec![template.clone()])
                .unwrap_err(),
            talos_fs::copy_file(&template, &paths.config_file).unwrap_err(),
        ),
        (
            save_config(&config, paths.config_file.to_str().unwrap()).unwrap_err(),
            talos_fs::std_fs()
                .write_file(&paths.config_file, &yaml)
                .unwrap_err(),
        ),
    ];
    for (propagated, original) in failures {
        assert_eq!(propagated.to_api_error(), original.to_api_error());
        assert!(std::error::Error::source(&propagated)
            .unwrap()
            .is::<std::io::Error>());
    }
}

fn write_resource_template(root: &Path, content: &str) -> PathBuf {
    let conf_dir = root.join("conf");
    std::fs::create_dir_all(&conf_dir).expect("resource conf dir should be created");
    let template_path = conf_dir.join("talos.yaml");
    std::fs::write(&template_path, content).expect("resource template should be written");
    root.to_path_buf()
}

#[test]
fn first_run_files_create_config_and_env_secret() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    let resource_dir = write_resource_template(
        dir.path(),
        "gateway:\n  auth: true\nllm:\n  anthropic:\n    api_key: ${TALOS_LLM_ANTHROPIC_API_KEY}\n",
    );

    paths
        .ensure_base_dirs()
        .expect("base dirs should be created");
    copy_default_config_template_from_candidates(
        &paths,
        vec![default_config_template_path(resource_dir)],
    )
    .expect("default config should copy");
    ensure_runtime_env_file(&paths).expect("env file should be created");

    let config =
        std::fs::read_to_string(&paths.config_file).expect("default config should be written");
    let env = std::fs::read_to_string(&paths.env_file).expect("env file should be written");
    assert!(config.contains("gateway:"));
    assert!(config.contains("auth: true"));
    assert!(config.contains("${TALOS_LLM_ANTHROPIC_API_KEY}"));
    assert!(env.is_empty());
}

#[test]
fn first_run_files_do_not_overwrite_existing_config_or_secret() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    let resource_dir = write_resource_template(dir.path(), "gateway:\n  auth: true\n");
    paths
        .ensure_base_dirs()
        .expect("base dirs should be created");
    std::fs::write(&paths.config_file, "gateway:\n  host: custom\n").expect("config should write");
    std::fs::write(&paths.env_file, "TALOS_CLOUD_API_TOKEN=existing\n").expect("env should write");

    if !paths.config_file.exists() {
        copy_default_config_template_from_candidates(
            &paths,
            vec![default_config_template_path(resource_dir)],
        )
        .expect("default config should copy");
    }
    ensure_runtime_env_file(&paths).expect("env should stay idempotent");

    let config = std::fs::read_to_string(&paths.config_file).expect("config should read");
    let env = std::fs::read_to_string(&paths.env_file).expect("env should read");
    assert_eq!(config, "gateway:\n  host: custom\n");
    assert_eq!(env, "TALOS_CLOUD_API_TOKEN=existing\n");
}

#[test]
fn first_run_config_copy_fails_when_resource_template_is_missing() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    paths
        .ensure_base_dirs()
        .expect("base dirs should be created");

    let err = copy_default_config_template_from_candidates(
        &paths,
        vec![dir.path().join("missing/conf/talos.yaml")],
    )
    .expect_err("missing resource template should fail");

    assert!(err.to_string().contains("missing default config template"));
    assert!(err.to_string().contains("TALOS_RESOURCE_DIR"));
}

#[test]
fn migration_install_replaces_only_the_managed_directory() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    paths
        .ensure_base_dirs()
        .expect("base dirs should be created");
    std::fs::write(&paths.config_file, "custom: true\n").expect("config should be written");

    let existing = paths.conf_dir.join("migrations").join("sqlite");
    std::fs::create_dir_all(&existing).expect("existing migrations should be created");
    std::fs::write(existing.join("obsolete.sql"), "SELECT 0;")
        .expect("obsolete migration should be written");

    let source = dir.path().join("release").join("conf").join("migrations");
    std::fs::create_dir_all(source.join("sqlite")).expect("sqlite source should be created");
    std::fs::create_dir_all(source.join("postgres")).expect("postgres source should be created");
    std::fs::write(source.join("sqlite").join("001_init.sql"), "SELECT 1;")
        .expect("sqlite migration should be written");
    std::fs::write(source.join("postgres").join("README.md"), "Reserved")
        .expect("postgres marker should be written");

    let report = replace_migration_resources_from_candidates(
        &paths,
        crate::DatabaseType::Sqlite,
        vec![source],
    )
    .expect("migration resources should install");
    let selected = report.backend_dir;

    assert_eq!(selected, paths.conf_dir.join("migrations").join("sqlite"));
    assert_eq!(
        report.source_dir,
        dir.path().join("release").join("conf").join("migrations")
    );
    assert_eq!(report.target_dir, paths.conf_dir.join("migrations"));
    assert_eq!(report.database_type, crate::DatabaseType::Sqlite);
    assert!(report.changed);
    assert_eq!(
        std::fs::read_to_string(&paths.config_file).expect("config should remain readable"),
        "custom: true\n"
    );
    assert!(!selected.join("obsolete.sql").exists());
    assert!(selected.join("001_init.sql").is_file());
    assert!(paths
        .conf_dir
        .join("migrations")
        .join("postgres")
        .join("README.md")
        .is_file());
}

#[test]
fn migration_install_returns_the_configured_backend_directory() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let paths = TalosPaths::from_home(dir.path().join(".talos"));
    let source = dir.path().join("release").join("conf").join("migrations");
    std::fs::create_dir_all(source.join("sqlite")).expect("sqlite source should be created");
    std::fs::create_dir_all(source.join("postgres")).expect("postgres source should be created");

    let report = replace_migration_resources_from_candidates(
        &paths,
        crate::DatabaseType::Postgres,
        vec![source],
    )
    .expect("postgres migration path should resolve");

    assert_eq!(
        report.backend_dir,
        paths.conf_dir.join("migrations").join("postgres")
    );
    assert_eq!(report.database_type, crate::DatabaseType::Postgres);
}
