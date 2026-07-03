use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use talos_config::{InMemoryLlmConfigStore, LlmConfigManager};
use talos_error::TalosError;
use talos_types::{ModelCompatConfig, ModelCostConfig, ModelInfo, ModelRef};

use super::ConfigService;

fn test_service() -> ConfigService {
    let llm = talos_config::LlmConfig::default();
    ConfigService::new(
        Arc::new(LlmConfigManager::new(Arc::new(
            InMemoryLlmConfigStore::new(llm),
        ))),
        vec![ModelInfo {
            id: "qwen-plus".to_string(),
            name: "Qwen Plus".to_string(),
            provider: "bailian".to_string(),
            context_window: 32_768,
            reasoning: true,
            reasoning_effort: Default::default(),
            speed: Default::default(),
            input: Vec::new(),
            cost: ModelCostConfig::default(),
            compat: ModelCompatConfig::default(),
        }],
    )
}

#[test]
fn validate_model_ref_supported_rejects_unknown_provider() {
    let service = test_service();

    let error = service
        .validate_model_ref_supported(&ModelRef {
            provider: "missing".to_string(),
            id: "unknown-model".to_string(),
        })
        .expect_err("unknown provider should be rejected");

    assert!(matches!(error, TalosError::UnsupportedModel(_)));
}

#[test]
fn get_llm_allows_regular_users() {
    let service = test_service();

    let llm = service
        .get_llm()
        .expect("regular user config read should work");

    assert_eq!(llm.provider, "anthropic");
}

#[test]
fn update_llm_invokes_reload_hook() {
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_for_hook = calls.clone();
    let service = ConfigService::new_with_reload_hook(
        Arc::new(LlmConfigManager::new(Arc::new(
            InMemoryLlmConfigStore::new(talos_config::LlmConfig::default()),
        ))),
        Vec::new(),
        Some(Arc::new(move |_| {
            calls_for_hook.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })),
    );

    let mut llm = service.get_llm().unwrap();
    llm.model = "updated-model".to_string();
    service.update_llm(llm).unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
