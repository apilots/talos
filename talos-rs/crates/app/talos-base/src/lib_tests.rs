use super::{
    apply_runtime_options, build_application_runtime_with_migration_dir, create_llm_provider,
    resolve_webui_dir, TalosServerOptions, WebuiLookupInput, WebuiLookupResult,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "server")]
use std::sync::Arc;
use talos_config::{
    AgentDefinitionConfig, AgentProviderConfig, Config, ProviderModelConfig, TalosPaths,
};
use talos_coordinator::AgentCoordinator;
use talos_llm_provider::ChatRequest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::Duration;

static TEMP_DIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn test_migrations_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite")
}

#[test]
fn resolve_webui_dir_uses_explicit_env_dir() {
    let root = temp_test_dir("webui-env");
    let webui = root.join("custom-webui");
    create_webui_bundle(&webui);

    let result = resolve_webui_dir(WebuiLookupInput {
        env_dir: Some(webui.clone()),
        current_exe: None,
        current_dir: None,
    });

    assert_eq!(result, WebuiLookupResult::Found(canonical(webui)));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolve_webui_dir_reports_invalid_explicit_env_dir() {
    let root = temp_test_dir("webui-invalid-env");
    let webui = root.join("missing-webui");

    let result = resolve_webui_dir(WebuiLookupInput {
        env_dir: Some(webui.clone()),
        current_exe: None,
        current_dir: None,
    });

    assert_eq!(result, WebuiLookupResult::InvalidEnv(webui));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolve_webui_dir_supports_installed_bin_sibling_layout() {
    let root = temp_test_dir("webui-installed");
    let bin = root.join("bin");
    let webui = root.join("webui");
    fs::create_dir_all(&bin).expect("bin dir should be created");
    create_webui_bundle(&webui);

    let result = resolve_webui_dir(WebuiLookupInput {
        env_dir: None,
        current_exe: Some(bin.join("talos-server")),
        current_dir: None,
    });

    assert_eq!(result, WebuiLookupResult::Found(canonical(webui)));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolve_webui_dir_supports_current_dir_webui_layout() {
    let root = temp_test_dir("webui-current-dir");
    let webui = root.join("webui");
    create_webui_bundle(&webui);

    let result = resolve_webui_dir(WebuiLookupInput {
        env_dir: None,
        current_exe: None,
        current_dir: Some(root.clone()),
    });

    assert_eq!(result, WebuiLookupResult::Found(canonical(webui)));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn resolve_webui_dir_returns_missing_without_candidates() {
    let root = temp_test_dir("webui-missing");

    let result = resolve_webui_dir(WebuiLookupInput {
        env_dir: None,
        current_exe: Some(root.join("bin").join("talos-server")),
        current_dir: Some(root.clone()),
    });

    assert_eq!(result, WebuiLookupResult::Missing);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn apply_runtime_options_updates_gateway_and_logging() {
    let config = Config::default();
    let options = TalosServerOptions {
        host: Some("127.0.0.1".to_string()),
        port: Some(18080),
        logging_stdout: Some(false),
        auth_disabled: None,
        enable_channels: None,
        webui_dir: None,
        session_ownership: None,
    };

    let updated = apply_runtime_options(config, Some(&options));

    assert_eq!(updated.gateway.host, "127.0.0.1");
    assert_eq!(updated.gateway.port, 18080);
    assert!(!updated.logging.stdout);
}

fn temp_test_dir(label: &str) -> PathBuf {
    let id = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("talos-base-{label}-{}-{id}", std::process::id()));
    fs::create_dir_all(&path).expect("temp test dir should be created");
    path
}

fn create_webui_bundle(path: &Path) {
    fs::create_dir_all(path).expect("webui dir should be created");
    fs::write(path.join("index.html"), "<!doctype html>").expect("index.html should be written");
}

fn canonical(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn isolate_runtime_workspace(config: &mut Config, label: &str) -> PathBuf {
    let workspace = temp_test_dir(label);
    config.agent.kernel.workspace_dir = workspace.to_string_lossy().to_string();
    *config = config
        .clone()
        .with_runtime_paths(TalosPaths::from_home(workspace.clone()));
    workspace
}

fn provider_model(id: &str) -> ProviderModelConfig {
    ProviderModelConfig {
        reasoning_replay: Default::default(),
        reasoning_efforts: None,
        request_parameters: Default::default(),
        id: id.to_string(),
        name: id.to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }
}

async fn spawn_json_server(body: impl Into<String>) -> (String, oneshot::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test server should bind");
    let addr = listener
        .local_addr()
        .expect("test server address should exist");
    let body = body.into();
    let (path_tx, path_rx) = oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("request should arrive");
        let mut buffer = vec![0; 4096];
        let read = socket.read(&mut buffer).await.expect("request should read");
        let request = String::from_utf8_lossy(&buffer[..read]);
        let path = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or_default()
            .to_string();
        path_tx.send(path).expect("path should send");
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("response should write");
    });
    (format!("http://{addr}"), path_rx)
}

#[tokio::test]
async fn application_runtime_builds_when_model_key_is_missing() {
    let mut config = Config::default();
    let workspace = isolate_runtime_workspace(&mut config, "runtime-missing-key");
    config.gateway.auth = false;
    config.storage.connection_string = ":memory:".to_string();
    config.llm.anthropic.models = vec![provider_model(&config.llm.model)];
    config.llm.anthropic.api_key = None;

    let runtime = build_application_runtime_with_migration_dir(config, &test_migrations_dir())
        .await
        .expect("runtime should start before model setup is complete");

    assert_eq!(runtime.config.llm.provider, "anthropic");
    runtime.shutdown().await.unwrap();
    let _ = fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn database_migration_failure_stops_before_product_upgrades_and_runtime() {
    let mut config = Config::default();
    let workspace = isolate_runtime_workspace(&mut config, "runtime-migration-failure");
    config.gateway.auth = false;
    config.storage.connection_string = ":memory:".to_string();
    let missing_migrations = workspace.join("missing-migrations");

    let error =
        match build_application_runtime_with_migration_dir(config, &missing_migrations).await {
            Ok(_) => panic!("database migration failure must stop startup"),
            Err(error) => error,
        };

    assert!(error.to_string().contains("load store migrations"));
    assert!(!workspace.join("upgrades/state.json").exists());
    assert!(!workspace.join("workspace-talos/anonymous").exists());
    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn runtime_config_rejects_invalid_profile_usage_retention() {
    let mut config = Config::default();
    config.automation.profile_usage_retention.retention_days = 0;

    let error = match config.validate_runtime() {
        Ok(_) => panic!("invalid Profile retention should fail runtime validation"),
        Err(error) => error,
    };

    assert!(error
        .to_string()
        .contains("profile_usage_retention.retention_days"));
}

#[tokio::test]
async fn application_runtime_registers_configured_default_agent() {
    let mut config = Config::default();
    let workspace = isolate_runtime_workspace(&mut config, "runtime-agent");
    config.gateway.auth = false;
    config.storage.connection_string = ":memory:".to_string();
    config.llm.model = "global-model".to_string();
    config.llm.anthropic.models = vec![provider_model("global-model")];
    config.agent.agents = vec![AgentDefinitionConfig {
        id: "reviewer".to_string(),
        name: "Reviewer".to_string(),
        enabled: true,
        description: Some("Review agent".to_string()),
        provider: Some(AgentProviderConfig::Kernel {
            kernel: config.agent.kernel.clone(),
        }),
    }];

    let runtime = build_application_runtime_with_migration_dir(config, &test_migrations_dir())
        .await
        .expect("runtime should build with configured agent");

    runtime
        .coordinator
        .ensure_agent("reviewer")
        .await
        .expect("configured agent should register");
    runtime.shutdown().await.unwrap();
    let _ = fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn application_runtime_skips_disabled_agent_definitions() {
    let mut config = Config::default();
    let workspace = isolate_runtime_workspace(&mut config, "runtime-disabled-agent");
    config.gateway.auth = false;
    config.storage.connection_string = ":memory:".to_string();
    config.llm.model = "global-model".to_string();
    config.llm.anthropic.models = vec![provider_model("global-model")];
    config.agent.agents = vec![
        AgentDefinitionConfig {
            id: "disabled".to_string(),
            name: "Disabled".to_string(),
            enabled: false,
            description: None,
            provider: None,
        },
        AgentDefinitionConfig {
            id: "enabled".to_string(),
            name: "Enabled".to_string(),
            enabled: true,
            description: None,
            provider: Some(AgentProviderConfig::Kernel {
                kernel: config.agent.kernel.clone(),
            }),
        },
    ];

    let runtime = build_application_runtime_with_migration_dir(config, &test_migrations_dir())
        .await
        .expect("runtime should build with enabled agent");

    assert!(runtime.coordinator.ensure_agent("disabled").await.is_err());
    assert!(runtime.coordinator.ensure_agent("enabled").await.is_ok());
    runtime.shutdown().await.unwrap();
    let _ = fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn setup_required_provider_reports_configuration_error_for_model_blocks() {
    let mut config = Config::default();
    config.llm.provider = "openai".to_string();
    config.llm.model = "missing-model".to_string();
    config.llm.openai.api_key = None;
    let provider = create_llm_provider(&config).unwrap();

    let result = provider
        .stream_model_blocks(ChatRequest::new_user("hello", "missing-model"))
        .await;
    let err = match result {
        Ok(_) => panic!("setup-required provider should fail"),
        Err(err) => err,
    };

    assert!(format!("{err}").contains("model setup required"));
}

#[tokio::test]
async fn openrouter_provider_uses_chat_completions_path_and_concrete_provider() {
    let body = r#"{"model":"test-model","choices":[{"message":{"content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
    let (base_url, path_rx) = spawn_json_server(body).await;

    let mut config = Config::default();
    config.llm.provider = "openrouter".to_string();
    config.llm.model = "test-model".to_string();
    config.llm.openrouter.api_key = Some("test-key".to_string());
    config.llm.openrouter.base_url = Some(base_url);
    config.llm.openrouter.models = vec![provider_model("test-model")];

    let provider = create_llm_provider(&config).expect("provider should be created");
    assert_eq!(provider.name(), "openrouter");
    assert_eq!(provider.default_model(), "test-model");

    let response = provider
        .chat(ChatRequest::new_user("hello", "test-model"))
        .await
        .expect("chat should succeed");
    let path = path_rx.await.expect("path should be captured");

    assert_eq!(path, "/chat/completions");
    assert_eq!(response.content, "ok");
}

#[tokio::test]
async fn openai_provider_uses_responses_without_protocol_configuration() {
    let body = r#"{"id":"resp_1","model":"gpt-test","status":"completed","output_text":"ok","output":[],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}"#;
    let (base_url, path_rx) = spawn_json_server(body).await;

    let mut config = Config::default();
    config.llm.provider = "openai".to_string();
    config.llm.model = "gpt-test".to_string();
    config.llm.openai.api_key = Some("test-key".to_string());
    config.llm.openai.base_url = Some(base_url);
    config.llm.openai.models = vec![provider_model("gpt-test")];

    let provider = create_llm_provider(&config).expect("provider should be created");
    assert_eq!(provider.name(), "openai");

    let response = provider
        .chat(ChatRequest::new_user("hello", "gpt-test"))
        .await
        .expect("chat should succeed");
    let path = path_rx.await.expect("path should be captured");

    assert_eq!(path, "/responses");
    assert_eq!(response.content, "ok");
}

#[tokio::test]
async fn deepseek_provider_uses_chat_completions_path_and_concrete_provider() {
    let body = r#"{"model":"deepseek-v4-pro","choices":[{"message":{"content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
    let (base_url, path_rx) = spawn_json_server(body).await;
    let mut config = Config::default();
    config.llm.provider = "deepseek".to_string();
    config.llm.model = "deepseek-v4-pro".to_string();
    config.llm.deepseek.api_key = Some("test-key".to_string());
    config.llm.deepseek.base_url = Some(base_url);
    config.llm.deepseek.models = vec![provider_model("deepseek-v4-pro")];

    let provider = create_llm_provider(&config).expect("provider should be created");
    let response = provider
        .chat(ChatRequest::new_user("hello", "deepseek-v4-pro"))
        .await
        .expect("chat should succeed");

    assert_eq!(provider.name(), "deepseek");
    assert_eq!(provider.default_model(), "deepseek-v4-pro");
    assert_eq!(
        path_rx.await.expect("path should be captured"),
        "/chat/completions"
    );
    assert_eq!(response.content, "ok");
}

#[cfg(unix)]
#[path = "application_mcp_tests.rs"]
mod mcp_tests;

#[path = "mcp_credentials_tests.rs"]
mod mcp_credentials_tests;

#[path = "mcp_oauth_tests.rs"]
mod mcp_oauth_tests;

#[cfg(all(feature = "relay", feature = "relay-client"))]
#[path = "product_relay_tests.rs"]
mod product_relay_tests;

#[tokio::test]
async fn product_management_assembly_does_not_open_the_live_worker_namespace() {
    use talos_fs::FileSystem;
    let mut config = Config::default();
    let workspace = isolate_runtime_workspace(&mut config, "product-only-management");
    config.gateway.auth = false;
    config.storage.connection_string = workspace.join("management.sqlite").to_string_lossy().into();
    let execution_dir = config.talos_paths().data_dir.join("worker");
    let lock = execution_dir.join("execution.lock");
    let executor_lock = talos_fs::std_fs().try_file_lock(&lock).unwrap().unwrap();
    let runtime =
        super::application_runtime::build_product_runtime(config.clone(), &test_migrations_dir())
            .await
            .unwrap();
    #[cfg(feature = "server")]
    assert!(runtime.service.nodes.server().is_err());
    assert!(!execution_dir.join("execution.sqlite").exists());
    assert!(!execution_dir.join("execution-admission").exists());
    // Session definition queries use product persistence even while another executor is live.
    runtime
        .service
        .session_automation(&talos_service::Actor::auth_disabled(), "missing")
        .await
        .unwrap_err();
    use talos_store::file::FileStore;
    let storage = super::application_storage::connect_application_storage(&config)
        .await
        .unwrap();
    let files = talos_store::file::DatabaseFileStore::new(&storage);
    files
        .create_file(talos_store::file::FileRecord {
            file_id: "live-upload".into(),
            owner_user_id: "owner".into(),
            project_id: None,
            location: talos_store::file::FileLocation::Local {
                storage_id: "local".into(),
                object_key: "files/live-upload/body".into(),
            },
            original_filename: "body.txt".into(),
            mime_type: None,
            size_bytes: None,
            status: talos_store::file::FileStatus::Uploading,
            tags: vec![],
            metadata: Default::default(),
            created_at: 1,
            updated_at: 1,
        })
        .await
        .unwrap();
    let second = super::application_runtime::build_product_runtime(config, &test_migrations_dir())
        .await
        .unwrap();
    let upload = files
        .get_file_for_owner(&talos_store::file::FileOwnerKey {
            file_id: "live-upload".into(),
            owner_user_id: "owner".into(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(upload.status, talos_store::file::FileStatus::Uploading);
    drop(second);
    drop(files);
    drop(storage);
    drop(runtime);
    drop(executor_lock);
    fs::remove_dir_all(workspace).unwrap();
}

#[tokio::test]
async fn personal_product_assembly_has_no_registration_or_managed_worker_authority() {
    let mut config = Config::default();
    let workspace = isolate_runtime_workspace(&mut config, "personal-no-control-plane");
    config.gateway.auth = false;
    config.storage.connection_string = workspace.join("personal.sqlite").to_string_lossy().into();
    config.llm.anthropic.models = vec![provider_model(&config.llm.model)];
    std::fs::create_dir_all(config.source_path().parent().unwrap()).unwrap();
    config.save(config.source_path().to_str().unwrap()).unwrap();
    let runtime = super::application_runtime::build_application_runtime_with_policy(
        config,
        &test_migrations_dir(),
        Default::default(),
        talos_types::NodeProductOwner::Computer,
    )
    .await
    .unwrap();
    #[cfg(feature = "server")]
    {
        assert!(runtime.node_service.is_none());
        #[cfg(feature = "server")]
        assert!(runtime.service.nodes.server().is_err());
    }
    let denied = runtime
        .service
        .nodes
        .create_node(
            &talos_service::Actor::regular("owner"),
            talos_types::CreateNodeRequest {
                display_name: "not-a-local-registration".into(),
                owner_user_id: None,
                endpoint: None,
                credential_expires_at: chrono::Utc::now().timestamp() + 60,
            },
        )
        .await
        .err()
        .expect("personal assembly cannot provision nodes");
    assert!(denied.is_code(talos_error::ErrorCode::Unauthenticated));
    let embedded = runtime
        .service
        .list_computers(&talos_service::Actor::auth_disabled(), Default::default())
        .await
        .unwrap();
    assert_eq!(embedded.data.len(), 1);
    assert_eq!(embedded.data[0].node_id, "embedded");
    runtime.shutdown().await.unwrap();
}

#[cfg(feature = "server")]
#[tokio::test]
async fn server_product_assembly_shares_control_plane_admission_with_product_operations() {
    let mut config = Config::default();
    let workspace = isolate_runtime_workspace(&mut config, "server-control-plane-gates");
    config.gateway.auth = false;
    config.storage.connection_string = workspace.join("server.sqlite").to_string_lossy().into();
    config.llm.anthropic.models = vec![provider_model(&config.llm.model)];
    let runtime = super::application_runtime::build_application_runtime_with_policy(
        config,
        &test_migrations_dir(),
        Default::default(),
        talos_types::NodeProductOwner::Server,
    )
    .await
    .unwrap();
    let server = runtime.node_service.as_ref().unwrap();
    assert!(runtime.service.nodes.server().is_ok());
    assert!(Arc::ptr_eq(
        &server.admission_gates(),
        &runtime.service.nodes.admission_gates()
    ));
    let read = server
        .admission_gates()
        .for_node("embedded")
        .read_owned()
        .await;
    let write = runtime.service.nodes.admission_gates().for_node("embedded");
    assert!(write.try_write().is_err());
    drop(read);
    assert!(write.try_write().is_ok());
    runtime.shutdown().await.unwrap();
}

#[cfg(not(feature = "server"))]
#[tokio::test]
async fn personal_build_rejects_server_composition_before_opening_storage() {
    let mut config = Config::default();
    let workspace = isolate_runtime_workspace(&mut config, "unsupported-server");
    let database = workspace.join("server.sqlite");
    config.storage.connection_string = database.to_string_lossy().into();
    let error = super::application_runtime::build_application_runtime_with_policy(
        config,
        &workspace.join("missing-migrations"),
        Default::default(),
        talos_types::NodeProductOwner::Server,
    )
    .await
    .err()
    .expect("personal build must not emulate Server authority");
    assert!(error.is_code(talos_error::ErrorCode::ServiceUnavailable));
    assert!(!database.exists());
}
