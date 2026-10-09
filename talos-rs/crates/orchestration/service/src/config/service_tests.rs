use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use talos_config::{ConfigStore, InMemoryLlmConfigStore};
use talos_storage::SqliteStorage;
use talos_store::session::{DatabaseSessionStore, MockSessionStore, SessionRecord, SessionStore};
use talos_types::{
    resolve_model_catalog, ConfigGetRequest, ConfigPath, ConfigReloadRequest, ConfigResponse,
    ConfigUpdateRequest, ConfigValue, ModelRef, ModelSelection, ReasoningEffort,
};

use super::{ConfigReloadPlan, ConfigReloadTrigger, ConfigService};

fn new_service(
    store: Arc<dyn talos_config::ConfigStore>,
    models: Vec<talos_types::ModelInfo>,
) -> ConfigService {
    ConfigService::new(
        store,
        models,
        Arc::new(MockSessionStore::new()),
        Arc::new(tokio::sync::RwLock::new(())),
        Arc::new(|_| {
            Ok(ConfigReloadPlan::new(
                talos_config::Config::default(),
                || {},
            ))
        }),
    )
}

fn new_service_with_reload_hook(
    store: Arc<dyn talos_config::ConfigStore>,
    models: Vec<talos_types::ModelInfo>,
    reload_hook: super::ConfigReloadHook,
) -> ConfigService {
    ConfigService::new(
        store,
        models,
        Arc::new(MockSessionStore::new()),
        Arc::new(tokio::sync::RwLock::new(())),
        reload_hook,
    )
}

fn test_service() -> ConfigService {
    let mut llm = talos_config::LlmConfig {
        model: "qwen-plus".to_string(),
        ..Default::default()
    };
    llm.anthropic.models = vec![talos_config::ProviderModelConfig {
        id: "qwen-plus".to_string(),
        name: "Qwen Plus".to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }];
    let models = resolve_model_catalog(&llm);
    new_service(Arc::new(InMemoryLlmConfigStore::new(llm)), models)
}

#[test]
fn validate_model_selection_supported_rejects_unknown_provider() {
    let service = test_service();

    let error = service
        .validate_model_selection_supported(&ModelSelection::without_reasoning(ModelRef {
            provider: "missing".to_string(),
            id: "unknown-model".to_string(),
        }))
        .expect_err("unknown provider should be rejected");

    assert!(error.is_code(talos_error::ErrorCode::ProviderUnavailable));
}

#[test]
fn validate_model_selection_supported_rejects_effort_for_unsupported_model() {
    let service = test_service();
    let selection = ModelSelection {
        model_ref: ModelRef {
            provider: "anthropic".to_string(),
            id: "qwen-plus".to_string(),
        },
        reasoning_effort: Some(ReasoningEffort::High),
    };

    let error = service
        .validate_model_selection_supported(&selection)
        .expect_err("unsupported reasoning effort should be rejected");

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
}

#[test]
fn get_llm_allows_regular_users() {
    let service = test_service();

    let llm = service
        .get_llm()
        .expect("regular user config read should work");

    assert_eq!(llm.provider, "anthropic");
}

#[tokio::test]
async fn list_models_returns_explicit_configured_default() {
    let service = test_service();

    let response = service
        .list_models()
        .await
        .expect("model catalog should load");

    assert_eq!(
        response.default_model_selection,
        ModelSelection::without_reasoning(ModelRef {
            provider: "anthropic".to_string(),
            id: "qwen-plus".to_string(),
        })
    );
    assert_eq!(response.models.len(), 1);
}

#[tokio::test]
async fn list_models_rejects_default_missing_from_provider_catalog() {
    let service = new_service(
        Arc::new(InMemoryLlmConfigStore::new(
            talos_config::LlmConfig::default(),
        )),
        Vec::new(),
    );

    let error = service
        .list_models()
        .await
        .expect_err("unconfigured default model should be rejected");

    assert!(error.is_code(talos_error::ErrorCode::ProviderUnavailable));
}

#[test]
fn validate_model_selection_uses_exact_provider_identity() {
    let service = test_service();

    let error = service
        .validate_model_selection_supported(&ModelSelection::without_reasoning(ModelRef {
            provider: "Anthropic".to_string(),
            id: "qwen-plus".to_string(),
        }))
        .expect_err("provider identity should be case-sensitive");

    assert!(error.is_code(talos_error::ErrorCode::ProviderUnavailable));
}

#[tokio::test]
async fn reload_config_invokes_explicit_runtime_reload() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_hook = calls.clone();
    let service = new_service_with_reload_hook(
        Arc::new(InMemoryLlmConfigStore::new(
            talos_config::LlmConfig::default(),
        )),
        Vec::new(),
        Arc::new(move |trigger| {
            assert!(matches!(trigger, ConfigReloadTrigger::Explicit));
            calls_for_hook.fetch_add(1, Ordering::SeqCst);
            Ok(ConfigReloadPlan::new(
                talos_config::Config::default(),
                || {},
            ))
        }),
    );

    let response = service.reload_config(ConfigReloadRequest {}).await.unwrap();

    assert!(response.success);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn get_config_reads_web_search_variant() {
    let service = test_service();

    let response = service
        .get_config(ConfigGetRequest {
            path: ConfigPath::WebSearch,
        })
        .expect("web search config should load");

    assert!(matches!(response, ConfigResponse::WebSearch { .. }));
}

#[tokio::test]
async fn update_config_saves_web_search_and_invokes_reload_hook() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_hook = calls.clone();
    let service = new_service_with_reload_hook(
        Arc::new(InMemoryLlmConfigStore::new(
            talos_config::LlmConfig::default(),
        )),
        Vec::new(),
        Arc::new(move |_| {
            calls_for_hook.fetch_add(1, Ordering::SeqCst);
            Ok(ConfigReloadPlan::new(
                talos_config::Config::default(),
                || {},
            ))
        }),
    );
    let web_search = talos_config::WebSearchToolConfig {
        provider: talos_config::WebSearchProviderConfig::Tavily,
        ..Default::default()
    };

    let response = service
        .update_config(ConfigUpdateRequest {
            value: ConfigValue::WebSearch(Box::new(web_search)),
        })
        .await
        .expect("web search config should save");

    match response {
        ConfigResponse::WebSearch { config } => {
            assert_eq!(
                config.provider,
                talos_config::WebSearchProviderConfig::Tavily
            );
        }
        other => panic!("unexpected response: {other:?}"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn update_config_rejects_nonterminal_turn_before_persistence_or_reload() {
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    crate::test_support::run_test_migrations(storage.pool()).await;
    let session_store = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
    session_store
        .insert_session_record(&SessionRecord {
            id: "session-config-gate".to_string(),
            agent_id: "default".to_string(),
            execution_node_id: "embedded".to_string(),
            tree_identity: talos_store::session::SessionTreeIdentity::Primary,
            workspace_id: "default".to_string(),
            title: talos_types::DEFAULT_SESSION_TITLE.to_string(),
            user_id: "member-user".to_string(),
            source_type: talos_types::talos::SessionSourceType::System,
            source_provider: talos_types::talos::SessionSourceProvider::System,
            channel_id: None,
            thread_id: None,
            environment: None,
            created_at: 1,
            last_activity: 1,
            message_count: 0,
            latest_turn_seq: 0,
            memory_processed_until_event_seq: 0,
            usage: Default::default(),
            conf: Default::default(),
            project_id: None,
            current_directory: None,
        })
        .await
        .unwrap();
    let identity = talos_agent_api::AgentTurnRef::new("session-config-gate", "turn-active");
    let message = crate::turn::completed_agent_user_input(
        &identity.turn_id,
        "message-active",
        crate::test_support::test_user_input("active"),
        1,
    )
    .unwrap();
    crate::test_support::commit_test_running_turn(session_store.as_ref(), identity, message).await;

    let config_store = Arc::new(InMemoryLlmConfigStore::new(
        talos_config::LlmConfig::default(),
    ));
    let before = config_store.load_web_search_read().unwrap();
    let prepared = Arc::new(AtomicUsize::new(0));
    let prepared_for_hook = prepared.clone();
    let service = ConfigService::new(
        config_store.clone(),
        Vec::new(),
        session_store,
        Arc::new(tokio::sync::RwLock::new(())),
        Arc::new(move |_| {
            prepared_for_hook.fetch_add(1, Ordering::SeqCst);
            Ok(ConfigReloadPlan::new(
                talos_config::Config::default(),
                || {},
            ))
        }),
    );
    let updated = talos_config::WebSearchToolConfig {
        provider: talos_config::WebSearchProviderConfig::Tavily,
        ..Default::default()
    };

    let error = service
        .update_config(ConfigUpdateRequest {
            value: ConfigValue::WebSearch(Box::new(updated)),
        })
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    assert_eq!(config_store.load_web_search_read().unwrap(), before);
    assert_eq!(prepared.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn update_config_rejects_non_http_web_search_base_url() {
    let service = test_service();
    let web_search = talos_config::WebSearchToolConfig {
        brave: talos_config::WebSearchProviderRuntimeConfig {
            base_url: Some("ftp://brave.example/search".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };

    let error = service
        .update_config(ConfigUpdateRequest {
            value: ConfigValue::WebSearch(Box::new(web_search)),
        })
        .await
        .expect_err("non-http base_url should be rejected");

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
}

#[tokio::test]
async fn update_config_rejects_invalid_web_search_base_url() {
    let service = test_service();
    let web_search = talos_config::WebSearchToolConfig {
        firecrawl: talos_config::WebSearchProviderRuntimeConfig {
            base_url: Some("not a url".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };

    let error = service
        .update_config(ConfigUpdateRequest {
            value: ConfigValue::WebSearch(Box::new(web_search)),
        })
        .await
        .expect_err("invalid base_url should be rejected");

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
}

#[tokio::test]
async fn update_config_rejects_web_search_base_url_without_host() {
    let service = test_service();
    for base_url in ["http:/foo", "http:///foo"] {
        let web_search = talos_config::WebSearchToolConfig {
            firecrawl: talos_config::WebSearchProviderRuntimeConfig {
                base_url: Some(base_url.to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let error = service
            .update_config(ConfigUpdateRequest {
                value: ConfigValue::WebSearch(Box::new(web_search)),
            })
            .await
            .expect_err("base_url without host should be rejected");

        assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
    }
}

#[tokio::test]
async fn update_config_rejects_zero_web_search_timeout() {
    let service = test_service();
    let web_search = talos_config::WebSearchToolConfig {
        tavily: talos_config::WebSearchProviderRuntimeConfig {
            timeout_ms: 0,
            ..Default::default()
        },
        ..Default::default()
    };

    let error = service
        .update_config(ConfigUpdateRequest {
            value: ConfigValue::WebSearch(Box::new(web_search)),
        })
        .await
        .expect_err("zero timeout should be rejected");

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
}
