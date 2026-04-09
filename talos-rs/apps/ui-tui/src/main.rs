use std::sync::Arc;

use dotenv::dotenv;
use talos_agent::{AgentImpl, AgentOrchestrator};
use talos_client::{ClientConfig, TalosClient};
use talos_config::Config;
use talos_coordinator::{Coordinator, CoordinatorImpl};
use talos_error::TalosResult;
use talos_gateway::{Gateway, GatewayImpl};
use talos_gateway_http::HttpServer;
use talos_llm_anthropic::AnthropicProvider;
use talos_llm_bailian::BailianProvider;
use talos_llm_openai::OpenAIProvider;
use talos_router::{Binding, MatchRule, Router, RouterImpl};
use talos_session_store::{DatabaseSessionStore, SessionStore};
use talos_storage::{create_storage, StorageConfig};
use talos_tools::{register_core_local_tools, register_network_tools, ToolRegistry};
use talos_types::talos::TalosConnectionMode;
use talos_types::{ModelInfo, RequestHandler, SessionService};
use talos_ui_tui::run_tui_app_with_client;
use tracing::error;

const DEFAULT_GATEWAY_URL: &str = "http://127.0.0.1:8080";
const DEFAULT_GATEWAY_TOKEN: &str = "talos-local-test-token";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let args: Vec<String> = std::env::args().collect();
    let use_external_gateway = has_flag(&args, "--gateway") || has_flag(&args, "-g");

    if use_external_gateway {
        let gateway_url = option_arg(&args, "--gateway-url")
            .or_else(|| std::env::var("TALOS_GATEWAY_URL").ok())
            .unwrap_or_else(|| DEFAULT_GATEWAY_URL.to_string());
        let auth_token = option_arg(&args, "--auth-token")
            .or_else(|| std::env::var("GATEWAY_TOKEN").ok())
            .or_else(|| Some(DEFAULT_GATEWAY_TOKEN.to_string()));

        eprintln!("Connecting to external gateway: {}", gateway_url);

        let config = ClientConfig {
            gateway_url,
            auth_token,
            timeout: 30,
        };
        let client = Arc::new(TalosClient::new(config)?);
        run_tui_app_with_client(client, TalosConnectionMode::Remote).await?;
    } else {
        run_embedded(args).await?;
    }

    Ok(())
}

async fn run_embedded(_args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::load().expect(
        "failed to load Talos config from <talos_home>/conf/talos.yaml — run `talos init` or use --gateway to connect to an external gateway",
    );
    config.validate_runtime().expect("invalid runtime config");
    let mut logging_config = config.logging.clone();
    // TUI owns stdout for terminal rendering; keep file logging but disable stdout logs here.
    logging_config.stdout = false;
    let _log_guard =
        talos_logging::init_with_config(logging_config).expect("failed to initialize logging");

    let storage_impl = create_storage(&StorageConfig {
        database_type: config.storage.database_type.clone(),
        connection_string: config.storage.connection_string.clone(),
        max_connections: config.storage.max_connections,
    })
    .await
    .expect("failed to create storage");
    let storage: Arc<dyn talos_storage::Storage> = Arc::from(storage_impl);
    let session_store: Arc<dyn SessionStore> = Arc::new(
        DatabaseSessionStore::new(storage)
            .await
            .expect("failed to initialize session store"),
    );

    let router: Arc<dyn Router> = Arc::new(RouterImpl::new());
    if let Err(e) = router
        .add_binding(Binding {
            id: "default_binding".to_string(),
            agent_id: "default".to_string(),
            rule: MatchRule::Default,
            priority: 1,
        })
        .await
    {
        error!("failed to create default routing: {}", e);
    }

    let llm_provider = create_llm_provider(&config)
        .await
        .expect("failed to create llm provider");
    let agent_llm_provider = llm_provider.clone();
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());
    register_network_tools(tool_registry.as_ref());

    let agent = AgentImpl::new(
        "default".to_string(),
        config.agent.name.clone(),
        "Default AI assistant".to_string(),
        agent_llm_provider,
        config.llm.model.clone(),
        tool_registry,
        config.agent.kernel.clone(),
    );
    let agent: Arc<dyn AgentOrchestrator> = Arc::new(agent);

    let mut coordinator_builder = CoordinatorImpl::new(router, session_store, config.clone())
        .with_default_workspace_id(config.agent.kernel.default_workspace_id.clone());
    coordinator_builder = coordinator_builder.with_session_title_generator(llm_provider.clone());
    let coordinator = Arc::new(coordinator_builder);
    coordinator
        .register_agent("default", agent)
        .await
        .expect("failed to register default agent");

    let request_handler: Arc<dyn RequestHandler> = coordinator.clone();
    let session_service: Arc<dyn SessionService> = coordinator.clone();
    let gateway: Arc<dyn Gateway> = Arc::new(GatewayImpl::new(request_handler));

    let port = find_free_port().await?;
    let default_model = config.llm.model.clone();
    let model_catalog = resolve_model_catalog(&config, &default_model);
    let http_server = HttpServer::new(
        gateway,
        session_service,
        "127.0.0.1",
        port,
        default_model,
        model_catalog,
    )
    .with_auth_disabled(true);

    // Start gateway in background
    tokio::spawn(async move {
        if let Err(e) = http_server.run().await {
            error!("embedded gateway error: {}", e);
        }
    });

    // Give the server a moment to bind
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let gateway_url = format!("http://127.0.0.1:{}", port);
    let client_config = ClientConfig {
        gateway_url,
        auth_token: None,
        timeout: 30,
    };
    let client = Arc::new(TalosClient::new(client_config)?);
    run_tui_app_with_client(client, TalosConnectionMode::Embedded).await?;

    Ok(())
}

async fn find_free_port() -> TalosResult<u16> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| talos_error::TalosError::NetworkError(e.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|e| talos_error::TalosError::NetworkError(e.to_string()))?
        .port();
    Ok(port)
}

async fn create_llm_provider(
    config: &Config,
) -> TalosResult<Arc<dyn talos_llm_provider::LlmProvider>> {
    use talos_error::TalosError;
    match config.llm.provider.as_str() {
        "anthropic" => {
            if config.llm.anthropic.enabled {
                let api_key = config.llm.anthropic.api_key.clone().ok_or_else(|| {
                    TalosError::InvalidConfiguration(
                        "llm.anthropic.api_key is required".to_string(),
                    )
                })?;
                return Ok(Arc::new(AnthropicProvider::new(
                    talos_llm_anthropic::AnthropicConfig::new(&api_key),
                )));
            }
        }
        "openai" => {
            if config.llm.openai.enabled {
                let api_key = config.llm.openai.api_key.clone().ok_or_else(|| {
                    TalosError::InvalidConfiguration("llm.openai.api_key is required".to_string())
                })?;
                let mut openai_cfg = talos_llm_openai::OpenAIConfig::new(&api_key);
                if let Some(base_url) = &config.llm.openai.base_url {
                    openai_cfg = openai_cfg.with_base_url(base_url);
                }
                return Ok(Arc::new(OpenAIProvider::new(openai_cfg)));
            }
        }
        "bailian" => {
            if config.llm.bailian.enabled {
                let api_key = config.llm.bailian.api_key.clone().ok_or_else(|| {
                    TalosError::InvalidConfiguration("llm.bailian.api_key is required".to_string())
                })?;
                let base_url = config.llm.bailian.base_url.clone().ok_or_else(|| {
                    TalosError::InvalidConfiguration("llm.bailian.base_url is required".to_string())
                })?;
                let bailian_cfg =
                    talos_llm_bailian::BailianConfig::new(&api_key).with_base_url(&base_url);
                return Ok(Arc::new(BailianProvider::new(bailian_cfg)));
            }
        }
        "ollama" => {
            if config.llm.ollama.enabled {
                let base_url = config.llm.ollama.base_url.clone().ok_or_else(|| {
                    TalosError::InvalidConfiguration("llm.ollama.base_url is required".to_string())
                })?;
                let openai_cfg =
                    talos_llm_openai::OpenAIConfig::new("ollama").with_base_url(&base_url);
                return Ok(Arc::new(OpenAIProvider::new(openai_cfg)));
            }
        }
        _ => {}
    }
    Err(talos_error::TalosError::InvalidConfiguration(format!(
        "unsupported or disabled provider '{}'",
        config.llm.provider
    )))
}

fn resolve_model_catalog(config: &Config, default_model: &str) -> Vec<ModelInfo> {
    let provider = config.llm.provider.clone();
    let provider_models = match config.llm.provider.as_str() {
        "anthropic" => config.llm.anthropic.models.clone(),
        "openai" => config.llm.openai.models.clone(),
        "bailian" => config.llm.bailian.models.clone(),
        "ollama" => config.llm.ollama.models.clone(),
        _ => Vec::new(),
    };
    let mut models: Vec<ModelInfo> = provider_models
        .into_iter()
        .filter(|m| !m.id.trim().is_empty())
        .map(|m| ModelInfo {
            id: m.id.clone(),
            name: m.display_name().to_string(),
            provider: provider.clone(),
            context_window: m.context_window,
            max_tokens: m.max_tokens,
            reasoning: m.reasoning,
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        })
        .collect();
    if !models.iter().any(|m| m.id == default_model) {
        models.push(ModelInfo {
            id: default_model.to_string(),
            name: default_model.to_string(),
            provider: config.llm.provider.clone(),
            context_window: 0,
            max_tokens: 0,
            reasoning: false,
            input: Vec::new(),
            cost: Default::default(),
            compat: Default::default(),
        });
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    models
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn option_arg(args: &[String], key: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == key {
            return iter.next().cloned();
        }
        if let Some((prefix, value)) = arg.split_once('=') {
            if prefix == key && !value.trim().is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}
