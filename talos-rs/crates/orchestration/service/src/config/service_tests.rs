use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use talos_config::{InMemoryLlmConfigStore, LlmConfigManager};
use talos_error::TalosError;
use talos_types::{
    ModelCostConfig, ModelInfo, ModelRef, ModelSelection, ReasoningEffort, ReasoningSelection,
};

use super::ConfigService;

fn test_service() -> ConfigService {
    let mut llm = talos_config::LlmConfig::default();
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
    ConfigService::new(
        Arc::new(LlmConfigManager::new(Arc::new(
            InMemoryLlmConfigStore::new(llm),
        ))),
        vec![ModelInfo {
            id: "qwen-plus".to_string(),
            name: "Qwen Plus".to_string(),
            provider: "anthropic".to_string(),
            context_window: 32_768,
            reasoning: true,
            reasoning_effort: Default::default(),
            speed: Default::default(),
            input: Vec::new(),
            cost: ModelCostConfig::default(),
        }],
    )
}

#[test]
fn validate_model_selection_supported_rejects_unknown_provider() {
    let service = test_service();

    let error = service
        .validate_model_selection_supported(&ModelSelection::model_default(ModelRef {
            provider: "missing".to_string(),
            id: "unknown-model".to_string(),
        }))
        .expect_err("unknown provider should be rejected");

    assert!(matches!(error, TalosError::UnsupportedModel(_)));
}

#[test]
fn validate_model_selection_supported_rejects_effort_for_unsupported_model() {
    let service = test_service();
    let selection = ModelSelection {
        model_ref: ModelRef {
            provider: "anthropic".to_string(),
            id: "qwen-plus".to_string(),
        },
        reasoning: ReasoningSelection::Effort {
            level: ReasoningEffort::High,
        },
    };

    let error = service
        .validate_model_selection_supported(&selection)
        .expect_err("unsupported reasoning effort should be rejected");

    assert!(matches!(error, TalosError::ValidationError(_)));
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
