use super::{
    apply_runtime_options, build_application_runtime_with_migration_dir, create_llm_provider,
    resolve_webui_dir, ModelSetupRequiredProvider, ReloadableLlmProvider, TalosServerHandle,
    TalosServerOptions, WebuiLookupInput, WebuiLookupResult, SERVER_SHUTDOWN_WAIT_TIMEOUT,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use talos_config::{
    AgentDefinitionConfig, AgentKind, AgentRuntimeConfig, Config, LlmProtocol, ProviderModelConfig,
};
use talos_gateway_http::{ServerLifecycle, ServerLifecycleState};
use talos_llm_provider::{ChatRequest, ChatResponse, ChatStream, LlmProvider, Usage};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;

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
    workspace
}

fn provider_model(id: &str) -> ProviderModelConfig {
    ProviderModelConfig {
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
        kind: AgentKind::Kernel,
        enabled: true,
        description: Some("Review agent".to_string()),
        default_workspace_id: Some("default".to_string()),
        runtime_config: Some(AgentRuntimeConfig::Kernel {
            kernel: config.agent.kernel.clone(),
        }),
    }];

    let runtime = build_application_runtime_with_migration_dir(config, &test_migrations_dir())
        .await
        .expect("runtime should build with configured agent");

    let registration = runtime
        .coordinator
        .registry()
        .get("reviewer")
        .await
        .expect("configured agent should register");
    assert_eq!(registration.descriptor.name, "Reviewer");
    assert_eq!(runtime.coordinator.registry().len().await, 1);
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
            kind: AgentKind::Kernel,
            enabled: false,
            description: None,
            default_workspace_id: None,
            runtime_config: None,
        },
        AgentDefinitionConfig {
            id: "enabled".to_string(),
            name: "Enabled".to_string(),
            kind: AgentKind::Kernel,
            enabled: true,
            description: None,
            default_workspace_id: None,
            runtime_config: Some(AgentRuntimeConfig::Kernel {
                kernel: config.agent.kernel.clone(),
            }),
        },
    ];

    let runtime = build_application_runtime_with_migration_dir(config, &test_migrations_dir())
        .await
        .expect("runtime should build with enabled agent");

    assert!(runtime
        .coordinator
        .registry()
        .get("disabled")
        .await
        .is_err());
    assert!(runtime.coordinator.registry().get("enabled").await.is_ok());
    assert_eq!(runtime.coordinator.registry().len().await, 1);
    runtime.shutdown().await.unwrap();
    let _ = fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn setup_required_provider_reports_configuration_error_for_model_blocks() {
    let provider = ModelSetupRequiredProvider {
        model: "missing-model".to_string(),
        message: "model setup required: missing key".to_string(),
    };

    let result = provider
        .stream_model_blocks(ChatRequest::new_user("hello", "missing-model"))
        .await;
    let err = match result {
        Ok(_) => panic!("setup-required provider should fail"),
        Err(err) => err,
    };

    assert!(format!("{err}").contains("model setup required: missing key"));
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
async fn openai_provider_uses_responses_path_when_protocol_selected() {
    let body = r#"{"id":"resp_1","model":"gpt-test","status":"completed","output_text":"ok","output":[],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}"#;
    let (base_url, path_rx) = spawn_json_server(body).await;

    let mut config = Config::default();
    config.llm.provider = "openai".to_string();
    config.llm.model = "gpt-test".to_string();
    config.llm.openai.api_key = Some("test-key".to_string());
    config.llm.openai.base_url = Some(base_url);
    config.llm.openai.protocol = Some(LlmProtocol::Responses);
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

#[tokio::test]
async fn reloadable_llm_provider_uses_reloaded_provider() {
    let provider = ReloadableLlmProvider::new(Arc::new(TestLlmProvider {
        text: "before".to_string(),
        model: "model-a".to_string(),
    }));

    let first = provider
        .chat(ChatRequest::new_user("hello", "model-a"))
        .await
        .expect("first provider should respond");
    provider.reload(Arc::new(TestLlmProvider {
        text: "after".to_string(),
        model: "model-b".to_string(),
    }));
    let second = provider
        .chat(ChatRequest::new_user("hello", "model-a"))
        .await
        .expect("reloaded provider should respond");

    assert_eq!(first.content, "before");
    assert_eq!(second.content, "after");
    assert_eq!(provider.default_model(), "model-b");
}

struct TestLlmProvider {
    text: String,
    model: String,
}

#[async_trait::async_trait]
impl LlmProvider for TestLlmProvider {
    fn name(&self) -> &str {
        "test"
    }

    async fn chat(&self, request: ChatRequest) -> talos_error::TalosResult<ChatResponse> {
        Ok(ChatResponse {
            content: self.text.clone(),
            reasoning: Default::default(),
            model: request.model,
            usage: Usage::default(),
            finish_reason: Some("stop".to_string()),
            tool_calls: None,
        })
    }

    async fn stream(&self, _request: ChatRequest) -> talos_error::TalosResult<ChatStream> {
        Err(talos_error::TalosError::internal(
            "stream is not used in this test".to_string(),
        ))
    }

    fn default_model(&self) -> String {
        self.model.clone()
    }
}

#[tokio::test]
async fn handle_shutdown_waits_for_stopped_state() {
    let lifecycle = Arc::new(ServerLifecycle::new());
    let handle = TalosServerHandle {
        shutdown_token: CancellationToken::new(),
        lifecycle: lifecycle.clone(),
        wait_timeout: Duration::from_secs(1),
    };

    let token = handle.shutdown_token.clone();
    let lifecycle_for_task = lifecycle.clone();
    let waiter = tokio::spawn(async move { handle.shutdown().await });
    token.cancelled().await;
    lifecycle_for_task.store(ServerLifecycleState::Quiescing);
    lifecycle_for_task.store(ServerLifecycleState::Draining);
    lifecycle_for_task.store(ServerLifecycleState::Stopped);

    waiter
        .await
        .expect("task should join")
        .expect("shutdown should finish");
}

#[tokio::test]
async fn handle_shutdown_times_out_without_stopped_state() {
    let lifecycle = Arc::new(ServerLifecycle::new());
    let handle = TalosServerHandle {
        shutdown_token: CancellationToken::new(),
        lifecycle,
        wait_timeout: Duration::from_millis(50),
    };

    let err = handle
        .shutdown()
        .await
        .expect_err("shutdown should time out");
    assert!(err.is_internal());
}

#[tokio::test]
async fn lifecycle_watch_reports_expected_state_order() {
    let lifecycle = Arc::new(ServerLifecycle::new());
    let mut updates = lifecycle.subscribe();

    lifecycle.store(ServerLifecycleState::Quiescing);
    updates
        .changed()
        .await
        .expect("quiescing update should arrive");
    assert_eq!(
        *updates.borrow_and_update(),
        ServerLifecycleState::Quiescing
    );

    lifecycle.store(ServerLifecycleState::Draining);
    updates
        .changed()
        .await
        .expect("draining update should arrive");
    assert_eq!(*updates.borrow_and_update(), ServerLifecycleState::Draining);

    lifecycle.store(ServerLifecycleState::Stopped);
    updates
        .changed()
        .await
        .expect("stopped update should arrive");
    assert_eq!(*updates.borrow_and_update(), ServerLifecycleState::Stopped);

    assert!(SERVER_SHUTDOWN_WAIT_TIMEOUT > Duration::from_secs(0));
}
