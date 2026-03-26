mod host;

use std::path::PathBuf;
use std::sync::Arc;

use dotenv::dotenv;
use talos_agent::{AgentImpl, AgentOrchestrator};
use talos_channel::{ChannelServer, TelegramChannel};
use talos_config::Config;
use talos_coordinator::{Coordinator, CoordinatorImpl};
use talos_error::TalosResult;
use talos_gateway::{Gateway, GatewayImpl};
use talos_gateway_http::HttpServer;
use talos_llm_anthropic::AnthropicProvider;
use talos_llm_openai::OpenAIProvider;
use talos_router::{Binding, MatchRule, Router, RouterImpl};
use talos_session_store::{DatabaseSessionStore, SessionStore};
use talos_storage::{create_storage, StorageConfig};
use talos_tools::{
    EditTool, ExecTool, MemoryGetTool, MemorySearchTool, ReadTool, ToolRegistry, WriteTool,
};
use talos_types::{RequestHandler, SessionService};
use tracing::{error, info};

use crate::host::TalosHost;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let config_source = resolve_default_config_path();
    let config = Config::load().expect("failed to load config");
    config.validate_runtime().expect("invalid runtime config");
    init_logging(&config).expect("failed to initialize logging");
    if let Some(path) = config_source {
        info!("Config loaded from {}", path.display());
    }
    info!(
        "Starting Talos server at {}:{}",
        config.gateway.host, config.gateway.port
    );

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
    let tool_registry = Arc::new(ToolRegistry::new());
    tool_registry.register(Arc::new(ReadTool));
    tool_registry.register(Arc::new(WriteTool));
    tool_registry.register(Arc::new(EditTool));
    tool_registry.register(Arc::new(ExecTool));
    tool_registry.register(Arc::new(MemorySearchTool));
    tool_registry.register(Arc::new(MemoryGetTool));

    let agent = AgentImpl::new(
        "default".to_string(),
        config.agent.name.clone(),
        "Default AI assistant".to_string(),
        llm_provider,
        config.llm.model.clone(),
        tool_registry.clone(),
        config.agent.kernel.clone(),
    );
    let agent: Arc<dyn AgentOrchestrator> = Arc::new(agent);

    let coordinator = Arc::new(
        CoordinatorImpl::new(router, session_store, config.clone())
            .with_channel_dm_scope("telegram", config.channels.telegram.dm_scope.clone())
            .with_default_workspace_id(config.agent.kernel.default_workspace_id.clone()),
    );
    coordinator
        .register_agent("default", agent)
        .await
        .expect("failed to register default agent");

    let request_handler: Arc<dyn RequestHandler> = coordinator.clone();
    let session_service: Arc<dyn SessionService> = coordinator.clone();
    let gateway: Arc<dyn Gateway> = Arc::new(GatewayImpl::new(request_handler.clone()));

    let mut channel_server = ChannelServer::new(Arc::new(config.clone()), request_handler);
    channel_server.register(Box::new(TelegramChannel::new()));

    let default_model = config.llm.model.clone();
    let model_provider = resolve_model_provider(&config);
    let supported_models = resolve_supported_models(&config, &default_model);
    let mut http_server = HttpServer::new(
        gateway,
        session_service,
        config.gateway.host.clone(),
        config.gateway.port,
        default_model,
        model_provider,
        supported_models,
    );
    if config.gateway.auth {
        http_server = http_server.with_jwt_secret(
            config
                .gateway
                .jwt_secret
                .clone()
                .expect("gateway.jwt_secret is required when gateway.auth=true"),
        );
    }
    if let Some(token) = config.gateway.token.clone() {
        http_server = http_server.with_static_token(token);
    }
    if !config.gateway.auth {
        http_server = http_server.with_auth_disabled(true);
    }

    let host = TalosHost::new().server(http_server).channel(channel_server);

    if let Err(e) = host.run().await {
        error!("host error: {}", e);
    }
}

fn resolve_default_config_path() -> Option<PathBuf> {
    if let Some(user_config_dir) = Config::user_config_dir() {
        let home_config = user_config_dir.join("talos.yaml");
        if home_config.exists() {
            return Some(canonical_or_original(home_config));
        }
    }

    let cwd_config = PathBuf::from("talos.yaml");
    if cwd_config.exists() {
        return Some(canonical_or_original(cwd_config));
    }

    None
}

fn canonical_or_original(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
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
                let openai_cfg =
                    talos_llm_openai::OpenAIConfig::new(&api_key).with_base_url(&base_url);
                return Ok(Arc::new(OpenAIProvider::new(openai_cfg)));
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

    Err(TalosError::InvalidConfiguration(format!(
        "unsupported or disabled provider '{}'",
        config.llm.provider
    )))
}

fn resolve_model_provider(config: &Config) -> String {
    config.llm.provider.clone()
}

fn resolve_provider_models(config: &Config) -> Vec<String> {
    let provider_models = match config.llm.provider.as_str() {
        "anthropic" => config.llm.anthropic.supported_models.clone(),
        "openai" => config.llm.openai.supported_models.clone(),
        "bailian" => config.llm.bailian.supported_models.clone(),
        "ollama" => config.llm.ollama.supported_models.clone(),
        _ => Vec::new(),
    };
    provider_models
}

fn resolve_supported_models(config: &Config, default_model: &str) -> Vec<String> {
    let mut models = vec![default_model.to_string()];
    models.extend(resolve_provider_models(config));
    if config.llm.model != default_model {
        models.push(config.llm.model.clone());
    }
    models.sort();
    models.dedup();
    models
}

fn init_logging(config: &Config) -> TalosResult<()> {
    talos_logging::init_with_config(config.logging.clone())?;
    Ok(())
}
