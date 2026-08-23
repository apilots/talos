use talos_types::{ModelInfo, ModelRef};

use super::resolve_inner_model_ref;

fn catalog() -> Vec<ModelInfo> {
    vec![
        ModelInfo::from_provider_model(
            "anthropic",
            serde_json::from_value(serde_json::json!({"id": "agent-default"})).unwrap(),
        ),
        ModelInfo::from_provider_model(
            "deepseek",
            serde_json::from_value(serde_json::json!({"id": "inner-mini"})).unwrap(),
        ),
    ]
}

#[test]
fn resolver_handles_explicit_default_and_unknown_models() {
    let default = ModelRef {
        provider: "anthropic".to_string(),
        id: "agent-default".to_string(),
    };
    let configured = ModelRef {
        provider: "deepseek".to_string(),
        id: "inner-mini".to_string(),
    };

    assert_eq!(
        (
            resolve_inner_model_ref(Some(&configured), &default, &catalog()).unwrap(),
            resolve_inner_model_ref(None, &default, &catalog()).unwrap(),
        ),
        (configured, default.clone())
    );
    let error = resolve_inner_model_ref(
        Some(&ModelRef {
            provider: "missing".to_string(),
            id: "unknown".to_string(),
        }),
        &default,
        &catalog(),
    )
    .unwrap_err();
    assert!(error.is_code(talos_error::ErrorCode::ProviderUnavailable));
}
