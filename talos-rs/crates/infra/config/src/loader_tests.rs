use super::*;

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
