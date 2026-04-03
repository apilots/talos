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
use talos_llm_bailian::BailianProvider;
use talos_llm_openai::OpenAIProvider;
use talos_logging::LogGuard;
use talos_router::{Binding, MatchRule, Router, RouterImpl};
use talos_session_store::{DatabaseSessionStore, SessionStore};
use talos_storage::{create_storage, StorageConfig};
use talos_tools::{
    register_collaboration_tools, register_core_local_tools, register_network_tools,
    CollaborationRuntime, ToolRegistry,
};
use talos_types::{ModelInfo, RequestHandler, SessionService};
use tracing::{error, info};

use crate::host::TalosHost;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let config_source = resolve_default_config_path();
    let config = Config::load().expect("failed to load config");
    config.validate_runtime().expect("invalid runtime config");
    let _log_guard = init_logging(&config).expect("failed to initialize logging");
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
    register_core_local_tools(tool_registry.as_ref());
    register_network_tools(tool_registry.as_ref());

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
    if config.agent.kernel.tools.enable_collaboration_tools {
        register_collaboration_tools(tool_registry.as_ref());
        tool_registry.set_collaboration_runtime(Arc::new(CollaborationRuntime::new(
            coordinator.clone(),
            coordinator.clone(),
        )));
    }
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
    let model_catalog = resolve_model_catalog(&config, &default_model);
    let mut http_server = HttpServer::new(
        gateway,
        session_service,
        config.gateway.host.clone(),
        config.gateway.port,
        default_model,
        model_catalog,
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
    let config_path = Config::default_path();
    config_path
        .exists()
        .then(|| canonical_or_original(config_path))
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

    Err(TalosError::InvalidConfiguration(format!(
        "unsupported or disabled provider '{}'",
        config.llm.provider
    )))
}

fn resolve_provider_models(config: &Config) -> Vec<ModelInfo> {
    let provider = config.llm.provider.clone();
    let provider_models = match config.llm.provider.as_str() {
        "anthropic" => config.llm.anthropic.models.clone(),
        "openai" => config.llm.openai.models.clone(),
        "bailian" => config.llm.bailian.models.clone(),
        "ollama" => config.llm.ollama.models.clone(),
        _ => Vec::new(),
    };
    provider_models
        .into_iter()
        .filter(|model| !model.id.trim().is_empty())
        .map(|model| ModelInfo {
            id: model.id.clone(),
            name: model.display_name().to_string(),
            provider: provider.clone(),
            context_window: model.context_window,
            max_tokens: model.max_tokens,
            reasoning: model.reasoning,
            input: model.input.clone(),
            cost: model.cost.clone(),
            compat: model.compat.clone(),
        })
        .collect()
}

fn resolve_model_catalog(config: &Config, default_model: &str) -> Vec<ModelInfo> {
    let mut models = resolve_provider_models(config);
    if !models.iter().any(|model| model.id == default_model) {
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
    if config.llm.model != default_model {
        let model_id = config.llm.model.clone();
        if !models.iter().any(|model| model.id == model_id) {
            models.push(ModelInfo {
                id: model_id.clone(),
                name: model_id,
                provider: config.llm.provider.clone(),
                context_window: 0,
                max_tokens: 0,
                reasoning: false,
                input: Vec::new(),
                cost: Default::default(),
                compat: Default::default(),
            });
        }
    }
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    models
}

fn init_logging(config: &Config) -> TalosResult<LogGuard> {
    talos_logging::init_with_config(config.logging.clone())
}
