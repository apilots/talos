use super::*;

#[test]
fn parse_dotenv_filters_non_talos_keys() {
    let parsed = parse_dotenv_filtered(
        r#"
TALOS_CLOUD_API_TOKEN=secret
ANTHROPIC_API_KEY=ignored
export TALOS_LLM_ANTHROPIC_API_KEY="sk-test"
INVALID-NAME=ignored
"#,
    );

    assert_eq!(
        parsed.get("TALOS_CLOUD_API_TOKEN").map(String::as_str),
        Some("secret")
    );
    assert_eq!(
        parsed
            .get("TALOS_LLM_ANTHROPIC_API_KEY")
            .map(String::as_str),
        Some("sk-test")
    );
    assert!(!parsed.contains_key("ANTHROPIC_API_KEY"));
}

#[test]
fn process_env_overrides_dotenv() {
    let dir = tempfile::tempdir().expect("temp dir should be created");
    let env_file = dir.path().join(".env");
    std::fs::write(&env_file, "TALOS_CLOUD_API_TOKEN=from-file\n").expect("env file should write");

    let env = RuntimeEnv::load_from_iter(&env_file, [("TALOS_CLOUD_API_TOKEN", "from-process")])
        .expect("runtime env should load");

    assert_eq!(env.get("TALOS_CLOUD_API_TOKEN"), Some("from-process"));
}
