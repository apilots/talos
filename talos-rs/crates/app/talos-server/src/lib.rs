use std::sync::Arc;

use talos_agent::{AgentImpl, AgentOrchestrator};
use talos_channel::{ChannelServer, TelegramChannel};
use talos_config::Config;
use talos_coordinator::{Coordinator, CoordinatorImpl};
use talos_error::{TalosError, TalosResult};
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
use tokio::task::JoinSet;
use tracing::error;

#[derive(Debug, Clone, Default)]
pub struct TalosServerOptions {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub logging_stdout: Option<bool>,
    pub auth_disabled: Option<bool>,
    pub enable_channels: Option<bool>,
}

pub struct TalosServerRuntime {
    http_server: HttpServer,
    channel_server: Option<ChannelServer>,
    gateway_url: String,
}

impl TalosServerRuntime {
    pub fn gateway_url(&self) -> &str {
        &self.gateway_url
    }

    pub fn into_http_server(self) -> TalosResult<HttpServer> {
        if self.channel_server.is_some() {
            return Err(TalosError::InvalidConfiguration(
                "cannot extract HTTP server while channels are enabled".to_string(),
            ));
        }
        Ok(self.http_server)
    }

    pub async fn run(self) -> TalosResult<()> {
        match self.channel_server {
            Some(channel_server) => {
                let http_server = self.http_server;
                let mut tasks = JoinSet::new();
                tasks.spawn(async move { http_server.run().await });
                tasks.spawn(async move { channel_server.run().await });

                match tasks.join_next().await {
                    Some(Ok(result)) => result,
                    Some(Err(err)) => Err(TalosError::InternalError(format!(
                        "server task join error: {}",
                        err
                    ))),
                    None => Ok(()),
                }
            }
            None => self.http_server.run().await,
        }
    }
}

pub fn init_logging(
    config: &Config,
    options: Option<&TalosServerOptions>,
) -> TalosResult<LogGuard> {
    let mut config = apply_runtime_options(config.clone(), options);
    if config.logging.timezone.is_none() {
        config.logging.timezone = Some(config.timezone.clone());
    }
    talos_logging::init_with_config(config.logging)
}

pub async fn start_server(
    config: Config,
    options: Option<TalosServerOptions>,
) -> TalosResult<TalosServerRuntime> {
    let config = apply_runtime_options(config, options.as_ref());

    let storage_impl = create_storage(&StorageConfig {
        database_type: config.storage.database_type.clone(),
        connection_string: config.storage.connection_string.clone(),
        max_connections: config.storage.max_connections,
    })
    .await?;
    let storage: Arc<dyn talos_storage::Storage> = Arc::from(storage_impl);
    let session_store: Arc<dyn SessionStore> = Arc::new(DatabaseSessionStore::new(storage).await?);

    let router: Arc<dyn Router> = Arc::new(RouterImpl::new());
    if let Err(err) = router
        .add_binding(Binding {
            id: "default_binding".to_string(),
            agent_id: "default".to_string(),
            rule: MatchRule::Default,
            priority: 1,
        })
        .await
    {
        error!("failed to create default routing: {}", err);
    }

    let llm_provider = create_llm_provider(&config).await?;
    let tool_registry = Arc::new(ToolRegistry::new());
    register_core_local_tools(tool_registry.as_ref());
    register_network_tools(tool_registry.as_ref());

    let agent = AgentImpl::new(
        "default".to_string(),
        config.agent.name.clone(),
        "Default AI assistant".to_string(),
        llm_provider.clone(),
        config.llm.model.clone(),
        tool_registry.clone(),
        config.agent.kernel.clone(),
    );
    let agent: Arc<dyn AgentOrchestrator> = Arc::new(agent);

    let mut coordinator_builder = CoordinatorImpl::new(router, session_store, config.clone())
        .with_channel_dm_scope("telegram", config.channels.telegram.dm_scope.clone())
        .with_default_workspace_id(config.agent.kernel.default_workspace_id.clone());
    coordinator_builder = coordinator_builder.with_session_title_generator(llm_provider.clone());
    let coordinator = Arc::new(coordinator_builder);

    if config.agent.kernel.tools.enable_collaboration_tools {
        register_collaboration_tools(tool_registry.as_ref());
        tool_registry.set_collaboration_runtime(Arc::new(CollaborationRuntime::new(
            coordinator.clone(),
            coordinator.clone(),
        )));
    }

    coordinator.register_agent("default", agent).await?;

    let request_handler: Arc<dyn RequestHandler> = coordinator.clone();
    let session_service: Arc<dyn SessionService> = coordinator.clone();
    let gateway: Arc<dyn Gateway> = Arc::new(GatewayImpl::new(request_handler.clone()));

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

    let auth_disabled = options
        .as_ref()
        .and_then(|value| value.auth_disabled)
        .unwrap_or(!config.gateway.auth);

    if auth_disabled {
        http_server = http_server.with_auth_disabled(true);
    } else {
        if config.gateway.auth {
            http_server = http_server.with_jwt_secret(
                config.gateway.jwt_secret.clone().ok_or_else(|| {
                    TalosError::InvalidConfiguration(
                        "gateway.jwt_secret is required when gateway.auth=true".to_string(),
                    )
                })?,
            );
        }
        if let Some(token) = config.gateway.token.clone() {
            http_server = http_server.with_static_token(token);
        }
    }

    let enable_channels = options
        .as_ref()
        .and_then(|value| value.enable_channels)
        .unwrap_or(true);
    let channel_server = if enable_channels {
        let mut channel_server = ChannelServer::new(Arc::new(config.clone()), request_handler);
        channel_server.register(Box::new(TelegramChannel::new()));
        Some(channel_server)
    } else {
        None
    };

    Ok(TalosServerRuntime {
        gateway_url: format!("http://{}:{}", config.gateway.host, config.gateway.port),
        http_server,
        channel_server,
    })
}

fn apply_runtime_options(mut config: Config, options: Option<&TalosServerOptions>) -> Config {
    let Some(options) = options else {
        return config;
    };

    if let Some(host) = &options.host {
        config.gateway.host = host.clone();
    }
    if let Some(port) = options.port {
        config.gateway.port = port;
    }
    if let Some(logging_stdout) = options.logging_stdout {
        config.logging.stdout = logging_stdout;
    }

    config
}

async fn create_llm_provider(
    config: &Config,
) -> TalosResult<Arc<dyn talos_llm_provider::LlmProvider>> {
    match config.llm.provider.as_str() {
        "anthropic" if config.llm.anthropic.enabled => {
            let api_key = config.llm.anthropic.api_key.clone().ok_or_else(|| {
                TalosError::InvalidConfiguration("llm.anthropic.api_key is required".to_string())
            })?;
            let base_url = config
                .llm
                .anthropic
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string());
            let anthropic_config = talos_llm_anthropic::AnthropicConfig::new(&api_key)
                .with_base_url(&base_url)
                .with_default_model(&config.llm.model);
            Ok(Arc::new(AnthropicProvider::new(anthropic_config)))
        }
        "openai" if config.llm.openai.enabled => {
            let api_key = config.llm.openai.api_key.clone().ok_or_else(|| {
                TalosError::InvalidConfiguration("llm.openai.api_key is required".to_string())
            })?;
            let mut openai_config = talos_llm_openai::OpenAIConfig::new(&api_key);
            if let Some(base_url) = &config.llm.openai.base_url {
                openai_config = openai_config.with_base_url(base_url);
            }
            Ok(Arc::new(OpenAIProvider::new(openai_config)))
        }
        "bailian" if config.llm.bailian.enabled => {
            let api_key = config.llm.bailian.api_key.clone().ok_or_else(|| {
                TalosError::InvalidConfiguration("llm.bailian.api_key is required".to_string())
            })?;
            let base_url = config.llm.bailian.base_url.clone().ok_or_else(|| {
                TalosError::InvalidConfiguration("llm.bailian.base_url is required".to_string())
            })?;
            let bailian_config =
                talos_llm_bailian::BailianConfig::new(&api_key).with_base_url(&base_url);
            Ok(Arc::new(BailianProvider::new(bailian_config)))
        }
        "ollama" if config.llm.ollama.enabled => {
            let base_url = config.llm.ollama.base_url.clone().ok_or_else(|| {
                TalosError::InvalidConfiguration("llm.ollama.base_url is required".to_string())
            })?;
            let openai_config =
                talos_llm_openai::OpenAIConfig::new("ollama").with_base_url(&base_url);
            Ok(Arc::new(OpenAIProvider::new(openai_config)))
        }
        _ => Err(TalosError::InvalidConfiguration(format!(
            "unsupported or disabled provider '{}'",
            config.llm.provider
        ))),
    }
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
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    models
}

#[cfg(test)]
mod tests {
    use super::{apply_runtime_options, TalosServerOptions};
    use talos_config::Config;

    #[test]
    fn apply_runtime_options_updates_gateway_and_logging() {
        let config = Config::default();
        let options = TalosServerOptions {
            host: Some("127.0.0.1".to_string()),
            port: Some(18080),
            logging_stdout: Some(false),
            auth_disabled: None,
            enable_channels: None,
        };

        let updated = apply_runtime_options(config, Some(&options));

        assert_eq!(updated.gateway.host, "127.0.0.1");
        assert_eq!(updated.gateway.port, 18080);
        assert!(!updated.logging.stdout);
    }
}
