use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use talos_config::{ConfigStore, InMemoryLlmConfigStore};
use talos_types::{
    ConfigGetRequest, ConfigPath, ConfigReloadRequest, ConfigResponse, ConfigUpdateRequest,
    ConfigValue,
};

use super::{ConfigReloadPlan, ConfigReloadTrigger, ConfigService};

fn new_service(store: Arc<dyn talos_config::ConfigStore>) -> ConfigService {
    ConfigService::new(
        store,
        Arc::new(tokio::sync::RwLock::new(())),
        Arc::new(|_| Ok(ConfigReloadPlan::new(|| {}))),
    )
}

fn new_service_with_reload_hook(
    store: Arc<dyn talos_config::ConfigStore>,
    reload_hook: super::ConfigReloadHook,
) -> ConfigService {
    ConfigService::new(store, Arc::new(tokio::sync::RwLock::new(())), reload_hook)
}

fn test_service() -> ConfigService {
    let mut llm = talos_config::LlmConfig {
        model: "qwen-plus".to_string(),
        ..Default::default()
    };
    llm.anthropic.models = vec![talos_config::ProviderModelConfig {
        reasoning_replay: Default::default(),
        reasoning_efforts: None,
        request_parameters: Default::default(),
        id: "qwen-plus".to_string(),
        name: "Qwen Plus".to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }];
    new_service(Arc::new(InMemoryLlmConfigStore::new(llm)))
}

#[tokio::test]
async fn reload_config_invokes_explicit_runtime_reload() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_hook = calls.clone();
    let service = new_service_with_reload_hook(
        Arc::new(InMemoryLlmConfigStore::new(
            talos_config::LlmConfig::default(),
        )),
        Arc::new(move |trigger| {
            assert!(matches!(trigger, ConfigReloadTrigger::Explicit));
            calls_for_hook.fetch_add(1, Ordering::SeqCst);
            Ok(ConfigReloadPlan::new(|| {}))
        }),
    );

    let response = service
        .reload_config(ConfigReloadRequest { node_id: None })
        .await
        .unwrap();

    assert!(response.success);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn get_config_reads_web_search_variant() {
    let service = test_service();

    let response = service
        .get_config(ConfigGetRequest {
            node_id: None,
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
        Arc::new(move |_| {
            calls_for_hook.fetch_add(1, Ordering::SeqCst);
            Ok(ConfigReloadPlan::new(|| {}))
        }),
    );
    let web_search = talos_config::WebSearchToolConfig {
        provider: talos_config::WebSearchProviderConfig::Tavily,
        ..Default::default()
    };

    let response = service
        .update_config(ConfigUpdateRequest {
            node_id: None,
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
async fn update_config_rejects_active_worker_before_persistence_or_reload() {
    let gate = Arc::new(tokio::sync::RwLock::new(()));
    let _execution = gate.clone().read_owned().await;
    let config_store = Arc::new(InMemoryLlmConfigStore::new(
        talos_config::LlmConfig::default(),
    ));
    let before = config_store.load_web_search_read().unwrap();
    let prepared = Arc::new(AtomicUsize::new(0));
    let prepared_for_hook = prepared.clone();
    let service = ConfigService::new(
        config_store.clone(),
        gate,
        Arc::new(move |_| {
            prepared_for_hook.fetch_add(1, Ordering::SeqCst);
            Ok(ConfigReloadPlan::new(|| {}))
        }),
    );
    let updated = talos_config::WebSearchToolConfig {
        provider: talos_config::WebSearchProviderConfig::Tavily,
        ..Default::default()
    };

    let error = service
        .update_config(ConfigUpdateRequest {
            node_id: None,
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
            node_id: None,
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
            node_id: None,
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
                node_id: None,
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
            node_id: None,
            value: ConfigValue::WebSearch(Box::new(web_search)),
        })
        .await
        .expect_err("zero timeout should be rejected");

    assert!(error.is_code(talos_error::ErrorCode::InvalidRequest));
}
